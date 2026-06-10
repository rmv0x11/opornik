//! CLI for training and benchmarking the long-nardy evaluation net.
//!
//! ```text
//! engine-train train [--games N] [--hidden H] [--lr L] [--explore E] [--seed S] [--out PATH]
//! engine-train bench  --in PATH [--games N] [--plies P] [--vs heuristic|random] [--hplies P]
//! ```

use std::io::BufRead;
use std::process::ExitCode;

use engine_core::bearoff::BearoffTable;
use engine_core::board::Board;
use engine_core::game::{outcome, Outcome};
use engine_core::moves::generate_turns_cfg;
use engine_core::net::Net;
use engine_core::player::Player;
use engine_core::search::{position_equity, Composite, Heuristic};
use engine_train::logasai::{self, Decision};
use engine_train::{
    benchmark, best_index, default_threads, format_pos_line, parse_player, parse_pos_line,
    parse_side, phase_of, rollout_equity, rollout_leaf_value, rollout_probs, sample_decisions,
    train_parallel, turn_key, BenchResult, Phase, Policy, RandomPolicy, Rng, SearchPolicy,
    TrainConfig,
};

fn load_net(path: &str) -> Option<Net> {
    let bytes = std::fs::read(path).ok()?;
    Net::from_bytes(&bytes)
}

/// Either net generation, behind one [`Evaluator`] — duels and label-scoring
/// work across formats (v1 `Net` vs v2 `PhaseNets`).
enum AnyEval {
    V1(Net),
    V2(engine_core::PhaseNets),
}

impl engine_core::search::Evaluator for AnyEval {
    fn equity(&self, board: &Board, mover: Player) -> f32 {
        match self {
            AnyEval::V1(n) => engine_core::search::Evaluator::equity(n, board, mover),
            AnyEval::V2(p) => engine_core::search::Evaluator::equity(p, board, mover),
        }
    }
}

/// Load a net of either format, sniffing the `NV2P` magic.
fn load_eval(path: &str) -> Option<AnyEval> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.starts_with(b"NV2P") {
        engine_core::PhaseNets::from_bytes(&bytes).map(AnyEval::V2)
    } else {
        Net::from_bytes(&bytes).map(AnyEval::V1)
    }
}

fn eval_kind(e: &AnyEval) -> &'static str {
    match e {
        AnyEval::V1(_) => "v1",
        AnyEval::V2(_) => "v2",
    }
}

/// Numeric/parseable flag value. A PRESENT flag with a missing or unparseable
/// value is a hard error — measurement runs must not silently revert to
/// defaults because of a typo (`--trials 24O`).
fn arg<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> T {
    match args.iter().position(|a| a == flag) {
        None => default,
        Some(i) => match args.get(i + 1).and_then(|v| v.parse().ok()) {
            Some(v) => v,
            None => {
                eprintln!(
                    "bad or missing value for {flag}: {:?}",
                    args.get(i + 1).map(String::as_str).unwrap_or("<none>")
                );
                std::process::exit(2);
            }
        },
    }
}

/// String flag value. A PRESENT flag followed by nothing or by another flag
/// is a hard error (`--save --per-phase` must not save to "--per-phase").
fn str_arg(args: &[String], flag: &str, default: &str) -> String {
    match args.iter().position(|a| a == flag) {
        None => default.to_string(),
        Some(i) => match args.get(i + 1) {
            Some(v) if !v.starts_with("--") => v.clone(),
            other => {
                eprintln!(
                    "bad or missing value for {flag}: {:?}",
                    other.map(String::as_str).unwrap_or("<none>")
                );
                std::process::exit(2);
            }
        },
    }
}

fn cmd_train(args: &[String]) -> ExitCode {
    let mut cfg = TrainConfig {
        hidden: arg(args, "--hidden", 80),
        games: arg(args, "--games", 20_000),
        lr: arg(args, "--lr", 0.05),
        explore: arg(args, "--explore", 0.05),
        seed: arg(args, "--seed", 0xC0FFEE),
        selfplay_plies: arg(args, "--selfplay-plies", 1u8),
        target_plies: arg(args, "--target-plies", 1u8),
    };
    if !(1..=2).contains(&cfg.target_plies) {
        // only the 1-ply bootstrap and the 2-ply lookahead target are implemented
        eprintln!("--target-plies must be 1 or 2 (got {})", cfg.target_plies);
        return ExitCode::FAILURE;
    }
    let out = str_arg(args, "--out", "net.bin");
    let threads = arg(args, "--threads", default_threads());
    let sync = arg(args, "--sync", 200);
    let init_path = str_arg(args, "--init", "");

    let init = if init_path.is_empty() {
        None
    } else {
        match load_net(&init_path) {
            Some(n) => {
                cfg.hidden = n.hidden; // warm-start must match the loaded shape
                println!("Warm-starting from {init_path} (hidden {})", n.hidden);
                Some(n)
            }
            None => {
                eprintln!("Failed to load --init net: {init_path}");
                return ExitCode::FAILURE;
            }
        }
    };

    println!(
        "Training: {} games, hidden {}, lr {}, explore {}, seed {}, threads {}, sync {}, \
         selfplay-plies {}, target-plies {}",
        cfg.games,
        cfg.hidden,
        cfg.lr,
        cfg.explore,
        cfg.seed,
        threads,
        sync,
        cfg.selfplay_plies,
        cfg.target_plies
    );
    let net = train_parallel(cfg, threads, sync, init, true);

    match std::fs::write(&out, net.to_bytes()) {
        Ok(()) => {
            println!("Saved net to {out} ({} bytes)", net.to_bytes().len());
            let np = SearchPolicy { eval: net, plies: 1 };
            let r = benchmark(&np, &SearchPolicy { eval: Heuristic, plies: 1 }, 200, 12345);
            report("net(1-ply) vs heuristic(1-ply)", &r);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Failed to write {out}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_duel(args: &[String]) -> ExitCode {
    let pa = str_arg(args, "--a", "");
    let pb = str_arg(args, "--b", "");
    let games = arg(args, "--games", 200);
    let plies = arg(args, "--plies", 1u8);
    let (na, nb) = match (load_eval(&pa), load_eval(&pb)) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            eprintln!("Failed to load nets: --a {pa} --b {pb}");
            return ExitCode::FAILURE;
        }
    };
    let label = format!(
        "A({pa} [{}]) vs B({pb} [{}]) @ {plies}-ply",
        eval_kind(&na),
        eval_kind(&nb)
    );
    let a = SearchPolicy { eval: na, plies };
    let b = SearchPolicy { eval: nb, plies };
    let r = benchmark(&a, &b, games, 555);
    report(&label, &r);
    ExitCode::SUCCESS
}

fn cmd_bench(args: &[String]) -> ExitCode {
    let path = str_arg(args, "--in", "net.bin");
    let games = arg(args, "--games", 200);
    let plies = arg(args, "--plies", 1u8);
    let hplies = arg(args, "--hplies", 1u8);
    let vs = str_arg(args, "--vs", "heuristic");

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let net = match Net::from_bytes(&bytes) {
        Some(n) => n,
        None => {
            eprintln!("Malformed net file: {path}");
            return ExitCode::FAILURE;
        }
    };

    let np = SearchPolicy { eval: net, plies };
    let opp: Box<dyn Policy> = match vs.as_str() {
        "random" => Box::new(RandomPolicy),
        _ => Box::new(SearchPolicy { eval: Heuristic, plies: hplies }),
    };
    let r = benchmark(&np, opp.as_ref(), games, 2024);
    report(&format!("net({plies}-ply) vs {vs}"), &r);
    ExitCode::SUCCESS
}

/// Relay / analysis: given a position and roll, rank the legal plays by the net's
/// equity (with `--plies` lookahead) and show win% for each. Useful for hints and
/// for manual sparring against an external engine (e.g. LogasAI).
fn cmd_analyze(args: &[String]) -> ExitCode {
    let net = match load_net(&str_arg(args, "--net", "")) {
        Some(n) => n,
        None => {
            eprintln!("analyze: need a valid --net PATH");
            return ExitCode::FAILURE;
        }
    };
    let mut board = Board::empty();
    parse_side(&str_arg(args, "--white", ""), Player::White, &mut board);
    parse_side(&str_arg(args, "--black", ""), Player::Black, &mut board);
    board.off[Player::White.index()] = arg(args, "--white-off", 0u8);
    board.off[Player::Black.index()] = arg(args, "--black-off", 0u8);

    let turn = parse_player(&str_arg(args, "--turn", "W"));
    let dice: Vec<u8> = str_arg(args, "--dice", "")
        .split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect();
    if dice.len() != 2 {
        eprintln!("analyze: need --dice d1,d2");
        return ExitCode::FAILURE;
    }
    let dice = [dice[0], dice[1]];
    let head = if str_arg(args, "--head", "").eq_ignore_ascii_case("unlimited") {
        None
    } else {
        Some(1u8)
    };
    let plies = arg(args, "--plies", 2u8).max(1);
    let top = arg(args, "--top", 5usize);

    if !board.is_valid() {
        eprintln!(
            "warning: not a 15+15 position (W {}+{} off, B {}+{} off)",
            board.checkers_on_board(Player::White),
            board.off[Player::White.index()],
            board.checkers_on_board(Player::Black),
            board.off[Player::Black.index()],
        );
    }

    let opp = turn.opponent();
    let turns = generate_turns_cfg(&board, turn, dice, false, head);
    println!(
        "{turn:?} to play {}-{} — {} legal play(s), {plies}-ply",
        dice[0],
        dice[1],
        turns.len()
    );
    if turns.iter().all(|t| t.moves.is_empty()) {
        println!("  (forced pass)");
        return ExitCode::SUCCESS;
    }

    let mut scored: Vec<(usize, f32, f32)> = turns
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let eq = -position_equity(&net, &t.board, opp, plies - 1, head);
            let win = 1.0 - net.evaluate_board(&t.board, opp).win;
            (i, eq, win)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (rank, (i, eq, win)) in scored.iter().take(top).enumerate() {
        let t = &turns[*i];
        let mv = if t.moves.is_empty() {
            "pass".to_string()
        } else {
            t.moves
                .iter()
                .map(|m| {
                    if m.bear_off {
                        format!("{}/off", m.from)
                    } else {
                        format!("{}/{}", m.from, m.to)
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let marker = if rank == 0 { "*" } else { " " };
        println!("  {marker}{}. {:<26} eq {:+.3}  win {:.1}%", rank + 1, mv, eq, win * 100.0);
    }
    ExitCode::SUCCESS
}

/// Measure the exact bear-off endgame database: net+bearoff vs the bare net.
fn cmd_endbench(args: &[String]) -> ExitCode {
    let net = match load_net(&str_arg(args, "--in", "")) {
        Some(n) => n,
        None => {
            eprintln!("endbench: need a valid --in PATH");
            return ExitCode::FAILURE;
        }
    };
    let games = arg(args, "--games", 300);
    let plies = arg(args, "--plies", 1u8);

    let t0 = std::time::Instant::now();
    println!("Building exact bear-off database (54,264 states)...");
    let table = BearoffTable::build();
    println!("  built in {:.1}s", t0.elapsed().as_secs_f32());

    let with = SearchPolicy {
        eval: Composite::new(net.clone(), table),
        plies,
    };
    let without = SearchPolicy { eval: net, plies };
    let r = benchmark(&with, &without, games, 4242);
    report(&format!("net+bearoff vs net @ {plies}-ply"), &r);
    ExitCode::SUCCESS
}

/// A decision with rollout-labelled equities for every legal turn.
struct LabeledDecision {
    board: Board,
    mover: Player,
    dice: [u8; 2],
    /// `turn_key -> equity` for the mover, from rollouts.
    eqs: Vec<(String, f32)>,
}

/// Score an evaluator's 1-ply and 2-ply choices against labelled decisions.
/// Returns per-phase accumulators keyed by `Phase`.
#[allow(clippy::type_complexity)]
fn score_against_labels<E: engine_core::search::Evaluator>(
    net: &E,
    decisions: &[LabeledDecision],
) -> (Vec<(Phase, f64, f64, usize, usize, usize)>, usize) {
    use std::collections::HashMap;
    let mut by_phase: HashMap<Phase, (f64, f64, usize, usize, usize)> = HashMap::new();
    let mut skipped = 0usize;
    for d in decisions {
        let turns = generate_turns_cfg(&d.board, d.mover, d.dice, false, Some(1));
        let lookup: std::collections::HashMap<&str, f32> =
            d.eqs.iter().map(|(k, e)| (k.as_str(), *e)).collect();
        let eqs: Option<Vec<f32>> = turns
            .iter()
            .map(|t| lookup.get(turn_key(t).as_str()).copied())
            .collect();
        let Some(eqs) = eqs else {
            skipped += 1; // labels don't cover this turn list (foreign file?)
            continue;
        };
        let (best_idx, best) = eqs.iter().enumerate().fold(
            (0usize, f32::NEG_INFINITY),
            |(bi, be), (i, &e)| if e > be { (i, e) } else { (bi, be) },
        );
        let i1 = best_index(net, &turns, d.mover, 0);
        let i2 = best_index(net, &turns, d.mover, 1);
        let slot = by_phase
            .entry(phase_of(&d.board, d.mover))
            .or_insert((0.0, 0.0, 0, 0, 0));
        slot.0 += f64::from((best - eqs[i1]).max(0.0));
        slot.1 += f64::from((best - eqs[i2]).max(0.0));
        slot.2 += usize::from(i1 == best_idx);
        slot.3 += usize::from(i2 == best_idx);
        slot.4 += 1;
    }
    let mut rows: Vec<_> = by_phase.into_iter().map(|(p, t)| (p, t.0, t.1, t.2, t.3, t.4)).collect();
    rows.sort_by_key(|r| r.0.name());
    (rows, skipped)
}

fn cmd_er(args: &[String]) -> ExitCode {
    // --load scores ANY evaluator (v1 net or v2 pair) against the saved
    // labels; fresh generation additionally needs a v1 net, whose 1-ply
    // policy drives sampling and rollouts.
    let eval = match load_eval(&str_arg(args, "--in", "")) {
        Some(e) => e,
        None => {
            eprintln!("er: need a valid --in PATH (v1 net or NV2P pair)");
            return ExitCode::FAILURE;
        }
    };
    let m = arg(args, "--positions", 300usize);
    let n = arg(args, "--trials", 120usize);
    let seed = arg(args, "--seed", 77u64);
    let stratify = arg(args, "--stratify", 0usize);
    let save = str_arg(args, "--save", "");
    let load = str_arg(args, "--load", "");
    let per_phase = args.iter().any(|a| a == "--per-phase");
    println!(
        "er config: positions {m}, trials {n}, seed {seed}, stratify {stratify}{}{}",
        if load.is_empty() { String::new() } else { format!(", load {load}") },
        if save.is_empty() { String::new() } else { format!(", save {save}") },
    );

    let t0 = std::time::Instant::now();

    // Obtain labelled decisions: from a saved eval-set (fast, no rollouts —
    // the fixed regression set), or freshly via self-play + rollouts.
    let decisions: Vec<LabeledDecision> = if !load.is_empty() {
        let Ok(text) = std::fs::read_to_string(&load) else {
            eprintln!("er: cannot read --load {load}");
            return ExitCode::FAILURE;
        };
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cols = line.split('\t');
            let Some(spec) = cols.next() else { continue };
            let Some((board, mover, Some(dice), _)) = parse_pos_line(spec) else {
                eprintln!("er: bad spec line skipped: {spec}");
                continue;
            };
            let eqs: Vec<(String, f32)> = cols
                .filter_map(|c| {
                    let (k, v) = c.rsplit_once(':')?;
                    Some((k.to_string(), v.parse().ok()?))
                })
                .collect();
            if !eqs.is_empty() {
                out.push(LabeledDecision { board, mover, dice, eqs });
            }
        }
        println!("Loaded {} labelled decisions from {load}", out.len());
        out
    } else {
        let AnyEval::V1(ref net) = eval else {
            eprintln!(
                "er: generating fresh labels requires a v1 net (its 1-ply policy \
                 drives sampling and rollouts); score v2 pairs via --load"
            );
            return ExitCode::FAILURE;
        };
        println!("Building bear-off table (rollout truncation)...");
        let table = BearoffTable::build();
        let positions = sample_decisions(net, m, seed, 8, stratify);

        let threads = default_threads();
        let chunk = positions.len().div_ceil(threads);
        let net_ref: &Net = net;
        let table = &table;
        let mut decisions: Vec<LabeledDecision> = Vec::with_capacity(positions.len());
        std::thread::scope(|s| {
            let handles: Vec<_> = positions
                .chunks(chunk)
                .enumerate()
                .map(|(ci, slice)| {
                    let offset = ci * chunk;
                    s.spawn(move || {
                        let mut out = Vec::with_capacity(slice.len());
                        for (j, (board, mover, dice)) in slice.iter().enumerate() {
                            // Seeded per POSITION, not per chunk: labels must not
                            // depend on this machine's core count via chunking.
                            let mut rng = Rng::new(
                                seed ^ ((offset + j + 1) as u64)
                                    .wrapping_mul(0x9E37_79B9_7F4A_7C15),
                            );
                            let opp = mover.opponent();
                            let turns = generate_turns_cfg(board, *mover, *dice, false, Some(1));
                            let eqs = turns
                                .iter()
                                .map(|t| {
                                    let (mean, _) =
                                        rollout_equity(net_ref, table, &t.board, opp, n, &mut rng);
                                    (turn_key(t), -mean)
                                })
                                .collect();
                            out.push(LabeledDecision {
                                board: board.clone(),
                                mover: *mover,
                                dice: *dice,
                                eqs,
                            });
                        }
                        out
                    })
                })
                .collect();
            for h in handles {
                decisions.extend(h.join().unwrap());
            }
        });

        if !save.is_empty() {
            // Quantize the in-memory labels to exactly what the file stores, so
            // the ER printed below is reproducible from a later --load.
            for d in &mut decisions {
                for (_, e) in &mut d.eqs {
                    *e = (*e * 10_000.0).round() / 10_000.0;
                }
            }
            let mut text = format!(
                "# meta: cmd=er net={} positions={} decisions={} trials={} seed={} stratify={}\n\
                 # opornik eval-set: pos-spec \\t turnkey:rollout-equity…  (one decision per line)\n",
                str_arg(args, "--in", ""),
                m,
                decisions.len(),
                n,
                seed,
                stratify,
            );
            for d in &decisions {
                text.push_str(&format_pos_line(&d.board, d.mover, Some(d.dice)));
                for (k, e) in &d.eqs {
                    text.push_str(&format!("\t{k}:{e:.4}"));
                }
                text.push('\n');
            }
            if let Err(e) = std::fs::write(&save, text) {
                eprintln!("er: failed to write --save {save}: {e}");
                return ExitCode::FAILURE;
            }
            println!("Saved {} labelled decisions to {save}", decisions.len());
        }
        decisions
    };

    // Score the net against the labels (cheap, net-only).
    let (rows, skipped) = score_against_labels(&eval, &decisions);
    let (mut er1, mut er2, mut a1, mut a2, mut cnt) = (0.0f64, 0.0f64, 0usize, 0usize, 0usize);
    for &(_, e1, e2, x1, x2, c) in &rows {
        er1 += e1;
        er2 += e2;
        a1 += x1;
        a2 += x2;
        cnt += c;
    }
    let c = cnt.max(1) as f64;
    println!(
        "\nError rate vs rollouts ({cnt} decisions{}, {:.1}s):",
        if skipped > 0 { format!(", {skipped} skipped") } else { String::new() },
        t0.elapsed().as_secs_f32()
    );
    println!(
        "  1-ply: ER {:.4} equity/move,  move agreement {:.1}%",
        er1 / c,
        100.0 * a1 as f64 / c
    );
    println!(
        "  2-ply: ER {:.4} equity/move,  move agreement {:.1}%",
        er2 / c,
        100.0 * a2 as f64 / c
    );
    if per_phase {
        println!("  by phase:");
        for (phase, e1, e2, x1, x2, k) in rows {
            let kf = k.max(1) as f64;
            println!(
                "    {:<8} n={:<4} ER1 {:.4} (agree {:>5.1}%)   ER2 {:.4} (agree {:>5.1}%)",
                phase.name(),
                k,
                e1 / kf,
                100.0 * x1 as f64 / kf,
                e2 / kf,
                100.0 * x2 as f64 / kf,
            );
        }
    }
    ExitCode::SUCCESS
}

/// Generate a rollout-labelled training dataset (the wildbg-style supervised
/// path): positions from the net's self-play, labels = outcome probabilities
/// `[win_oin, win_mars, lose_oin, lose_mars]` from truncated rollouts.
fn cmd_dataset(args: &[String]) -> ExitCode {
    let net = match load_net(&str_arg(args, "--net", "")) {
        Some(n) => n,
        None => {
            eprintln!("dataset: need a valid --net PATH");
            return ExitCode::FAILURE;
        }
    };
    let out = str_arg(args, "--out", "dataset.tsv");
    let m = arg(args, "--positions", 10_000usize);
    let trials = arg(args, "--trials", 432usize);
    let seed = arg(args, "--seed", 4242u64);
    let explore = arg(args, "--explore", 0.03f32);
    let exclude = str_arg(args, "--exclude", "");
    println!(
        "dataset config: positions {m}, trials {trials}, seed {seed}, explore {explore}{}",
        if exclude.is_empty() { String::new() } else { format!(", exclude {exclude}") },
    );

    // Positions to keep OUT of the training data (the fixed eval-set): the
    // same net's near-greedy self-play revisits the same openings regardless
    // of the dice seed, so without an explicit exclusion the regression set
    // leaks into training.
    let mut excluded: std::collections::HashSet<(Board, Player)> = Default::default();
    if !exclude.is_empty() {
        let Ok(text) = std::fs::read_to_string(&exclude) else {
            eprintln!("dataset: cannot read --exclude {exclude}");
            return ExitCode::FAILURE;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let spec = line.split('\t').next().unwrap_or(line);
            if let Some((board, mover, _, _)) = parse_pos_line(spec) {
                excluded.insert((board, mover));
            }
        }
        println!("Excluding {} eval-set positions from the dataset", excluded.len());
    }

    let t0 = std::time::Instant::now();
    println!("Building bear-off table (rollout truncation)...");
    let table = BearoffTable::build();

    // Collect unique to-roll states from self-play (greedy + explore noise) —
    // the same distribution the net sees as search leaves. States where the
    // to-roll player still holds first-turn rights are skipped: rollout labels
    // are computed with `first=false`, and the head-doubles exception would
    // make such labels mismatch the position actually faced.
    let mut rng = Rng::new(seed);
    let mut seen: std::collections::HashSet<(Board, Player)> = std::collections::HashSet::new();
    let mut states: Vec<(Board, Player)> = Vec::with_capacity(m);
    while states.len() < m {
        let mut board = Board::starting();
        let mut mover = Player::White;
        let mut first = [true, true];
        for _ in 0..600 {
            if !matches!(outcome(&board), Outcome::Ongoing) {
                break;
            }
            let dice = rng.dice();
            let turns = generate_turns_cfg(&board, mover, dice, first[mover.index()], Some(1));
            first[mover.index()] = false;
            let idx = if explore > 0.0 && rng.unit() < explore {
                (rng.next_u64() as usize) % turns.len()
            } else {
                best_index(&net, &turns, mover, 0)
            };
            board = turns[idx].board.clone();
            mover = mover.opponent();
            if matches!(outcome(&board), Outcome::Ongoing)
                && !first[mover.index()]
                && !excluded.contains(&(board.clone(), mover))
                && seen.insert((board.clone(), mover))
            {
                states.push((board.clone(), mover));
                if states.len() >= m {
                    break;
                }
            }
        }
    }
    println!(
        "Collected {} unique to-roll states ({:.1}s); rolling out {} trials each…",
        states.len(),
        t0.elapsed().as_secs_f32(),
        trials
    );

    // Label in parallel.
    let threads = default_threads();
    let chunk = states.len().div_ceil(threads);
    let net_ref = &net;
    let table = &table;
    let mut rows: Vec<String> = Vec::with_capacity(states.len());
    let mut phase_counts: std::collections::HashMap<&'static str, usize> =
        std::collections::HashMap::new();
    std::thread::scope(|s| {
        let handles: Vec<_> = states
            .chunks(chunk)
            .enumerate()
            .map(|(ci, slice)| {
                let offset = ci * chunk;
                s.spawn(move || {
                    let mut out = Vec::with_capacity(slice.len());
                    for (j, (board, to_roll)) in slice.iter().enumerate() {
                        // per-position seed: labels independent of core count
                        let mut rng = Rng::new(
                            seed ^ ((offset + j + 1) as u64)
                                .wrapping_mul(0x9E37_79B9_7F4A_7C15),
                        );
                        let p = rollout_probs(net_ref, table, board, *to_roll, trials, &mut rng);
                        let phase = phase_of(board, *to_roll);
                        out.push((
                            format!(
                                "{}\t{}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{}",
                                format_pos_line(board, *to_roll, None),
                                phase.name(),
                                p[0],
                                p[1],
                                p[2],
                                p[3],
                                trials
                            ),
                            phase.name(),
                        ));
                    }
                    out
                })
            })
            .collect();
        for h in handles {
            for (row, phase) in h.join().unwrap() {
                rows.push(row);
                *phase_counts.entry(phase).or_insert(0) += 1;
            }
        }
    });

    let header = format!(
        "# meta: cmd=dataset net={} positions={} trials={} seed={} explore={} exclude={}\n\
         # opornik dataset: pos-spec \\t phase \\t p_win_oin \\t p_win_mars \\t p_lose_oin \\t p_lose_mars \\t trials\n",
        str_arg(args, "--net", ""),
        rows.len(),
        trials,
        seed,
        explore,
        if exclude.is_empty() { "-" } else { &exclude },
    );
    let body: String = rows.iter().map(|r| format!("{r}\n")).collect();
    if let Err(e) = std::fs::write(&out, format!("{header}{body}")) {
        eprintln!("dataset: failed to write {out}: {e}");
        return ExitCode::FAILURE;
    }
    let mut hist: Vec<_> = phase_counts.into_iter().collect();
    hist.sort();
    println!(
        "Wrote {} labelled positions to {out} ({:.1}s). Phase mix: {}",
        rows.len(),
        t0.elapsed().as_secs_f32(),
        hist.iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    ExitCode::SUCCESS
}

/// Diagnostic for the 2-ply null result: does 2-ply help once the LEAVES are
/// scored by rollouts instead of the net? Compares the ER (vs high-trial
/// rollout truth) of four choosers: net 1-ply, net 2-ply, 2-ply with
/// rollout-evaluated leaves, and a NOISE-FLOOR chooser that picks by an
/// independent same-budget re-rollout of the truth. The floor calibrates how
/// much regret pure rollout noise produces under this exact protocol — every
/// regret is charged against noisy truth, and noise-driven choosers pay a
/// winner's-curse tax the deterministic net choosers don't. The verdict
/// therefore compares EXCESS over the floor: if rollout leaves recover most of
/// the net-2-ply excess, the net's leaf errors are systematic
/// (encoding/training problem) — averaging over 21 rolls cannot cancel them.
fn cmd_diag2(args: &[String]) -> ExitCode {
    let net = match load_net(&str_arg(args, "--net", "")) {
        Some(n) => n,
        None => {
            eprintln!("diag2: need a valid --net PATH");
            return ExitCode::FAILURE;
        }
    };
    let m = arg(args, "--positions", 50usize);
    let truth_trials = arg(args, "--trials", 96usize);
    let leaf_trials = arg(args, "--leaf-trials", 24usize);
    let seed = arg(args, "--seed", 99u64);
    println!("diag2 config: positions {m}, truth-trials {truth_trials}, leaf-trials {leaf_trials}, seed {seed}");

    let t0 = std::time::Instant::now();
    println!("Building bear-off table (rollout truncation)...");
    let table = BearoffTable::build();
    let positions = sample_decisions(&net, m, seed, 8, 0);

    let threads = default_threads();
    let chunk = positions.len().div_ceil(threads);
    let net_ref = &net;
    let table = &table;
    // per-thread partials:
    // ([er1, er2, er_leaf, er_floor], sumsq per chooser, [a1..a_floor],
    //  paired (r2 - r_leaf) sum, its sumsq, cnt)
    type Partial = ([f64; 4], [f64; 4], [usize; 4], f64, f64, usize);
    let mut partials: Vec<Partial> = Vec::new();
    std::thread::scope(|s| {
        let handles: Vec<_> = positions
            .chunks(chunk)
            .enumerate()
            .map(|(ci, slice)| {
                let offset = ci * chunk;
                s.spawn(move || {
                    let mut ers = [0.0f64; 4];
                    let mut sq = [0.0f64; 4];
                    let mut agrees = [0usize; 4];
                    let (mut pair, mut pairsq) = (0.0f64, 0.0f64);
                    let mut cnt = 0usize;
                    for (j, (board, mover, dice)) in slice.iter().enumerate() {
                        // per-position seed: results independent of core count
                        let mut rng = Rng::new(
                            seed ^ ((offset + j + 1) as u64)
                                .wrapping_mul(0x9E37_79B9_7F4A_7C15),
                        );
                        let opp = mover.opponent();
                        let turns = generate_turns_cfg(board, *mover, *dice, false, Some(1));

                        // Truth: high-trial rollout equity of every candidate.
                        let eqs: Vec<f32> = turns
                            .iter()
                            .map(|t| {
                                -rollout_equity(net_ref, table, &t.board, opp, truth_trials, &mut rng).0
                            })
                            .collect();
                        let (best_idx, best) = eqs.iter().enumerate().fold(
                            (0usize, f32::NEG_INFINITY),
                            |(bi, be), (i, &e)| if e > be { (i, e) } else { (bi, be) },
                        );

                        // Noise floor: an independent re-rollout of the same
                        // budget picks its own argmax; its regret vs truth is
                        // what rollout noise alone costs under this protocol.
                        let floor_idx = turns
                            .iter()
                            .enumerate()
                            .map(|(i, t)| {
                                (i, -rollout_equity(net_ref, table, &t.board, opp, truth_trials, &mut rng).0)
                            })
                            .fold((0usize, f32::NEG_INFINITY), |(bi, be), (i, e)| {
                                if e > be { (i, e) } else { (bi, be) }
                            })
                            .0;

                        // Chooser 3: 2-ply with rollout leaves (terminal-safe).
                        let mut best_rl = (0usize, f32::NEG_INFINITY);
                        for (i, t) in turns.iter().enumerate() {
                            let val = rollout_leaf_value(
                                net_ref,
                                table,
                                &t.board,
                                *mover,
                                leaf_trials,
                                &mut rng,
                            );
                            if val > best_rl.1 {
                                best_rl = (i, val);
                            }
                        }

                        let i1 = best_index(net_ref, &turns, *mover, 0);
                        let i2 = best_index(net_ref, &turns, *mover, 1);
                        let mut regrets = [0.0f64; 4];
                        for (slot, idx) in [i1, i2, best_rl.0, floor_idx].into_iter().enumerate() {
                            let r = f64::from((best - eqs[idx]).max(0.0));
                            regrets[slot] = r;
                            ers[slot] += r;
                            sq[slot] += r * r;
                            agrees[slot] += usize::from(idx == best_idx);
                        }
                        let d = regrets[1] - regrets[2]; // net-2ply minus rollout-leaf
                        pair += d;
                        pairsq += d * d;
                        cnt += 1;
                    }
                    (ers, sq, agrees, pair, pairsq, cnt)
                })
            })
            .collect();
        for h in handles {
            partials.push(h.join().unwrap());
        }
    });

    let (ers, sq, agrees, pair, pairsq, cnt) = partials.into_iter().fold(
        ([0.0f64; 4], [0.0f64; 4], [0usize; 4], 0.0f64, 0.0f64, 0usize),
        |(mut e, mut q, mut a, ps, pq, c), p| {
            for k in 0..4 {
                e[k] += p.0[k];
                q[k] += p.1[k];
                a[k] += p.2[k];
            }
            (e, q, a, ps + p.3, pq + p.4, c + p.5)
        },
    );
    let c = cnt.max(1) as f64;
    let mean = ers.map(|e| e / c);
    let se: Vec<f64> = (0..4)
        .map(|k| ((sq[k] / c - mean[k] * mean[k]).max(0.0) / c).sqrt())
        .collect();
    let [er1, er2, erl, erf] = mean;
    let pct = |a: usize| 100.0 * a as f64 / c;
    println!(
        "\n2-ply leaf diagnostic ({cnt} decisions, truth {truth_trials} trials, leaves {leaf_trials} trials, {:.0}s):",
        t0.elapsed().as_secs_f32()
    );
    println!("  net 1-ply:           ER {er1:.4} ±{:.4}  agree {:.1}%", se[0], pct(agrees[0]));
    println!("  net 2-ply:           ER {er2:.4} ±{:.4}  agree {:.1}%", se[1], pct(agrees[1]));
    println!("  2-ply rollout-leaf:  ER {erl:.4} ±{:.4}  agree {:.1}%", se[2], pct(agrees[2]));
    println!(
        "  noise floor:         ER {erf:.4} ±{:.4}  agree {:.1}%   (same-budget re-rollout argmax)",
        se[3],
        pct(agrees[3])
    );
    // Paired per-decision difference: the cleanest signal — both choosers are
    // charged against the SAME truth realization, so the winner's-curse bias
    // cancels and only selection quality remains.
    let pmean = pair / c;
    let pse = ((pairsq / c - pmean * pmean).max(0.0) / c).sqrt();
    let z = if pse > 0.0 { pmean / pse } else { 0.0 };
    println!(
        "  paired (net2ply − rollout-leaf): {pmean:+.4} ± {pse:.4}  (z = {z:+.2})"
    );
    let ex2 = er2 - erf;
    let exl = erl - erf;
    println!(
        "  excess over floor:   net 2-ply {ex2:+.4}   rollout-leaf {exl:+.4}"
    );
    println!(
        "  → verdict: {}",
        if z >= 2.0 {
            "rollout leaves SIGNIFICANTLY outperform net leaves at 2-ply → the net's leaf errors are SYSTEMATIC (encoding/training limit)"
        } else if ex2 <= 0.005 && z.abs() < 2.0 {
            "inconclusive: net 2-ply is within noise of the rollout floor and the paired difference is not significant (raise --trials/--positions)"
        } else if exl < 0.4 * ex2 {
            "rollout leaves recover most of the 2-ply excess → the net's leaf errors are SYSTEMATIC (encoding/training limit)"
        } else {
            "rollout leaves do not fix 2-ply → leaf accuracy is not the binding constraint"
        }
    );
    ExitCode::SUCCESS
}

/// Convert a rollout-labelled dataset TSV into the binary feature matrices the
/// PyTorch trainer consumes: one file per phase net, routed by the SAME
/// dispatch the in-play evaluator uses (`has_contact` → contact, else race —
/// bear-off rows train the race net's fallback). Binary layout (little-endian):
/// `b"NNF2"  u32 version=1  u32 dim  u32 n`, then `n` rows of
/// `dim` f32 features + 4 f32 target probs. `train/train_v2.py` reads this.
fn cmd_encode(args: &[String]) -> ExitCode {
    use engine_core::encoding2::{encode_contact_into, encode_race_into, CONTACT_INPUTS, RACE_INPUTS};
    use engine_core::has_contact;

    let input = str_arg(args, "--in", "");
    let prefix = str_arg(args, "--out-prefix", "");
    if input.is_empty() || prefix.is_empty() {
        eprintln!("encode: need --in dataset.tsv --out-prefix PATH");
        return ExitCode::FAILURE;
    }
    let Ok(text) = std::fs::read_to_string(&input) else {
        eprintln!("encode: cannot read {input}");
        return ExitCode::FAILURE;
    };

    let mut contact_rows: Vec<f32> = Vec::new();
    let mut race_rows: Vec<f32> = Vec::new();
    let (mut n_contact, mut n_race, mut bad) = (0usize, 0usize, 0usize);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        // pos-spec, phase, p_win_oin, p_win_mars, p_lose_oin, p_lose_mars, trials
        if cols.len() < 6 {
            bad += 1;
            continue;
        }
        let (Some((board, mover, _, _)), Ok(p0), Ok(p1), Ok(p2), Ok(p3)) = (
            parse_pos_line(cols[0]),
            cols[2].parse::<f32>(),
            cols[3].parse::<f32>(),
            cols[4].parse::<f32>(),
            cols[5].parse::<f32>(),
        ) else {
            bad += 1;
            continue;
        };
        if has_contact(&board) {
            let at = contact_rows.len();
            contact_rows.resize(at + CONTACT_INPUTS, 0.0);
            encode_contact_into(&board, mover, &mut contact_rows[at..]);
            contact_rows.extend_from_slice(&[p0, p1, p2, p3]);
            n_contact += 1;
        } else {
            let at = race_rows.len();
            race_rows.resize(at + RACE_INPUTS, 0.0);
            encode_race_into(&board, mover, &mut race_rows[at..]);
            race_rows.extend_from_slice(&[p0, p1, p2, p3]);
            n_race += 1;
        }
    }

    let write = |path: &str, dim: usize, n: usize, rows: &[f32]| -> std::io::Result<()> {
        let mut out = Vec::with_capacity(16 + rows.len() * 4);
        out.extend_from_slice(b"NNF2");
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(dim as u32).to_le_bytes());
        out.extend_from_slice(&(n as u32).to_le_bytes());
        for v in rows {
            out.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(path, out)
    };
    let cpath = format!("{prefix}-contact.bin");
    let rpath = format!("{prefix}-race.bin");
    if let Err(e) = write(&cpath, CONTACT_INPUTS, n_contact, &contact_rows)
        .and_then(|()| write(&rpath, RACE_INPUTS, n_race, &race_rows))
    {
        eprintln!("encode: write failed: {e}");
        return ExitCode::FAILURE;
    }
    println!(
        "Encoded {input}: {n_contact} contact rows → {cpath} (dim {CONTACT_INPUTS}), \
         {n_race} race rows → {rpath} (dim {RACE_INPUTS}){}",
        if bad > 0 { format!(", {bad} bad lines skipped") } else { String::new() }
    );
    ExitCode::SUCCESS
}

/// Forward-pass speed: champion v1 net vs the v2 phase nets (stage-1 gate:
/// the v2 stack must stay within the play-time budget).
fn cmd_netbench(args: &[String]) -> ExitCode {
    use engine_core::net2::{NetV2, PhaseNets, OUTPUTS_V2};
    use engine_core::search::Evaluator;
    use engine_core::{CONTACT_INPUTS, RACE_INPUTS};

    let iters = arg(args, "--iters", 20_000usize);
    let v1 = load_net(&str_arg(args, "--in", "models/nardy-net.bin"))
        .unwrap_or_else(|| Net::standard(80, 1));
    let v2_path = str_arg(args, "--v2", "");
    let v2 = if v2_path.is_empty() {
        PhaseNets::new(
            NetV2::random(&[CONTACT_INPUTS, 300, 250, 200, OUTPUTS_V2], 11),
            NetV2::random(&[RACE_INPUTS, 300, 250, 200, OUTPUTS_V2], 22),
        )
        .expect("valid shapes")
    } else {
        let Some(p) = std::fs::read(&v2_path).ok().and_then(|b| PhaseNets::from_bytes(&b))
        else {
            eprintln!("netbench: cannot load NV2P pair from {v2_path}");
            return ExitCode::FAILURE;
        };
        println!("loaded v2 pair from {v2_path}");
        p
    };

    // Interop probe: the same synthetic input train/train_v2.py prints after
    // export — the two outputs must match to ~1e-6 or the export is broken
    // (e.g. transposed weights produce garbage without crashing).
    let probe = |dim: usize| -> Vec<f32> {
        (0..dim).map(|i| ((i * 7) % 23) as f32 / 23.0).collect()
    };
    println!("probe contact: {:?}", v2.contact.forward(&probe(CONTACT_INPUTS)));
    println!("probe race:    {:?}", v2.race.forward(&probe(RACE_INPUTS)));

    let board = Board::starting();
    let bench = |label: &str, f: &dyn Fn() -> f32| {
        let t0 = std::time::Instant::now();
        let mut acc = 0.0f32;
        for _ in 0..iters {
            acc += f();
        }
        let dt = t0.elapsed();
        println!(
            "  {label:<32} {:>8.2} µs/eval  ({iters} iters, checksum {acc:.3})",
            dt.as_secs_f64() * 1e6 / iters as f64
        );
        dt
    };
    let dims: Vec<String> = std::iter::once(v2.contact.input().to_string())
        .chain(v2.contact.layers.iter().map(|l| l.output.to_string()))
        .collect();
    let v2_label = format!("v2 PhaseNets {}", dims.join("-"));
    println!("Forward-pass benchmark:");
    let t1 = bench("v1 Net 196-80-3", &|| v1.equity(&board, Player::White));
    let t2 = bench(&v2_label, &|| v2.equity(&board, Player::White));
    println!(
        "  ratio: v2 is {:.1}× slower per eval (plan budget: ~10× absorbed by top-4 root pruning + SIMD)",
        t2.as_secs_f64() / t1.as_secs_f64()
    );
    ExitCode::SUCCESS
}

/// Relay: read position+roll lines from stdin and print the engine's best plays.
/// Pair with LogasAI — type its position and roll, mirror our move, record the
/// result. One position per line; blank lines and `#` comments are skipped.
fn cmd_relay(args: &[String]) -> ExitCode {
    let net = match load_net(&str_arg(args, "--net", "")) {
        Some(n) => n,
        None => {
            eprintln!("relay: need a valid --net PATH");
            return ExitCode::FAILURE;
        }
    };
    let plies = arg(args, "--ply", 2u8).max(1);
    let top = arg(args, "--top", 4usize);
    eprintln!("relay ready ({plies}-ply). line: white=24:13,.. black=24:15 turn=W dice=3,1 [first]");
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let line = line.trim().to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((board, mover, Some(dice), first)) = parse_pos_line(&line) else {
            eprintln!("  ? unparseable line (need white=/black= and dice=d1,d2)");
            continue;
        };
        if !board.is_valid() {
            eprintln!(
                "  ! not a 15+15 position (W {}, B {})",
                board.checkers_on_board(Player::White) + board.off[Player::White.index()],
                board.checkers_on_board(Player::Black) + board.off[Player::Black.index()],
            );
        }
        let ranked = logasai::rank(&net, &board, mover, dice, first, plies);
        if ranked.is_empty() || (ranked.len() == 1 && ranked[0].notation == "pass") {
            println!("{mover:?} {}-{}: forced pass", dice[0], dice[1]);
            continue;
        }
        println!(
            "{mover:?} to play {}-{} ({} plays, {plies}-ply):",
            dice[0],
            dice[1],
            ranked.len()
        );
        for (i, r) in ranked.iter().take(top).enumerate() {
            let mark = if i == 0 { "*" } else { " " };
            println!(
                "  {mark}{}. {:<26} eq {:+.3}  win {:.1}%",
                i + 1,
                r.notation,
                r.equity,
                r.win * 100.0
            );
        }
    }
    ExitCode::SUCCESS
}

/// Agreement / ER benchmark against logged games — the standard backgammon
/// strength metric vs a reference engine. Decisions come from a text file
/// (`--file`, one `… play=<move>` line each) and/or a `.MAT` transcript (`--mat`).
fn cmd_agree(args: &[String]) -> ExitCode {
    let net = match load_net(&str_arg(args, "--net", "")) {
        Some(n) => n,
        None => {
            eprintln!("agree: need a valid --net PATH");
            return ExitCode::FAILURE;
        }
    };
    let plies = arg(args, "--ply", 2u8).max(1);
    let mut decisions: Vec<Decision> = Vec::new();

    let file = str_arg(args, "--file", "");
    if !file.is_empty() {
        match std::fs::read_to_string(&file) {
            Ok(text) => {
                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let Some((prefix, played)) = line.split_once(" play=") else {
                        eprintln!("  ? line without `play=`: {line}");
                        continue;
                    };
                    if let Some((board, mover, Some(dice), first)) = parse_pos_line(prefix) {
                        decisions.push(Decision {
                            board,
                            mover,
                            dice,
                            first,
                            played: played.trim().to_string(),
                        });
                    }
                }
            }
            Err(e) => {
                eprintln!("agree: cannot read {file}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    let mat = str_arg(args, "--mat", "");
    if !mat.is_empty() {
        match std::fs::read_to_string(&mat) {
            Ok(text) => {
                let plies_in = logasai::parse_mat(&text);
                let (recon, fail) = logasai::replay_plies(&plies_in);
                println!(
                    "MAT {mat}: {} plies parsed, {} reconstructed{}",
                    plies_in.len(),
                    recon.len(),
                    fail.map(|k| format!(" (stopped at ply {k} — notation mismatch)"))
                        .unwrap_or_default(),
                );
                decisions.extend(recon);
            }
            Err(e) => {
                eprintln!("agree: cannot read {mat}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    if decisions.is_empty() {
        eprintln!("agree: no decisions (provide --file and/or --mat)");
        return ExitCode::FAILURE;
    }

    let r = logasai::agree(&net, &decisions, plies);
    println!(
        "\nAgreement vs our {plies}-ply best ({} decisions, {} matched, {} unmatched):",
        r.decisions, r.matched, r.unmatched
    );
    println!("  move agreement {:.1}%", r.agreement_rate() * 100.0);
    println!("  error rate     {:.4} equity/decision", r.error_rate());
    ExitCode::SUCCESS
}

fn report(label: &str, r: &BenchResult) {
    let (lo, hi) = r.ci95();
    let p = r.p_value_vs_even();
    let verdict = if p > 0.05 {
        "= even (not significant)"
    } else if r.win_rate() > 0.5 {
        "A is stronger"
    } else {
        "B is stronger"
    };
    println!(
        "{label}: {} games — win rate {:.1}% ({}/{}) | {:+.3} ppg | 95% CI [{:.1}%, {:.1}%] | p={:.3} → {}",
        r.games,
        r.win_rate() * 100.0,
        r.wins,
        r.games,
        r.ppg(),
        lo * 100.0,
        hi * 100.0,
        p,
        verdict
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("train") => cmd_train(&args[1..]),
        Some("bench") => cmd_bench(&args[1..]),
        Some("duel") => cmd_duel(&args[1..]),
        Some("analyze") => cmd_analyze(&args[1..]),
        Some("endbench") => cmd_endbench(&args[1..]),
        Some("er") => cmd_er(&args[1..]),
        Some("dataset") => cmd_dataset(&args[1..]),
        Some("encode") => cmd_encode(&args[1..]),
        Some("netbench") => cmd_netbench(&args[1..]),
        Some("diag2") => cmd_diag2(&args[1..]),
        Some("relay") => cmd_relay(&args[1..]),
        Some("agree") => cmd_agree(&args[1..]),
        _ => {
            eprintln!(
                "usage:\n  engine-train train [--games N] [--hidden H] [--lr L] [--explore E] [--seed S] [--threads T] [--sync K] [--init PATH] [--out PATH] [--selfplay-plies P] [--target-plies P]\n  engine-train bench --in PATH [--games N] [--plies P] [--vs heuristic|random] [--hplies P]\n  engine-train duel --a PATH --b PATH [--games N] [--plies P]\n  engine-train analyze --net PATH --white \"24:13,18:2\" --black \"24:15\" --dice 3,1 --turn W [--plies P] [--head unlimited] [--white-off N] [--black-off N] [--top N]\n  engine-train er --in PATH [--positions M] [--trials N] [--seed S] [--stratify K] [--per-phase] [--save FILE | --load FILE]\n  engine-train dataset --net PATH --out FILE [--positions M] [--trials N] [--seed S] [--explore E] [--exclude EVALSET]   (rollout-labelled training data)\n  engine-train encode --in dataset.tsv --out-prefix PATH   (TSV → NNF2 feature matrices for train/train_v2.py)\n  engine-train netbench [--in PATH] [--iters N]   (v1 vs v2 forward-pass speed)\n  engine-train diag2 --net PATH [--positions M] [--trials N] [--leaf-trials K] [--seed S]   (does 2-ply help with rollout leaves?)\n  engine-train relay --net PATH [--ply P] [--top N]   (reads position lines from stdin → best plays)\n  engine-train agree --net PATH [--file decisions.txt] [--mat match.MAT] [--ply P]   (move-agreement % + ER vs LogasAI)"
            );
            ExitCode::FAILURE
        }
    }
}
