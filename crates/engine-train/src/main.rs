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
use engine_core::moves::{generate_turns_cfg, Turn};
use engine_core::net::Net;
use engine_core::player::Player;
use engine_core::search::{position_equity, Composite, Heuristic};
use engine_train::logasai::{self, Decision};
use engine_train::{
    benchmark, default_threads, rollout_equity, train_parallel, BenchResult, Policy, RandomPolicy,
    Rng, SearchPolicy, TrainConfig,
};

fn load_net(path: &str) -> Option<Net> {
    let bytes = std::fs::read(path).ok()?;
    Net::from_bytes(&bytes)
}

fn arg<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> T {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn str_arg(args: &[String], flag: &str, default: &str) -> String {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| default.to_string())
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
    let (na, nb) = match (load_net(&pa), load_net(&pb)) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            eprintln!("Failed to load nets: --a {pa} --b {pb}");
            return ExitCode::FAILURE;
        }
    };
    let a = SearchPolicy { eval: na, plies };
    let b = SearchPolicy { eval: nb, plies };
    let r = benchmark(&a, &b, games, 555);
    report(&format!("A({pa}) vs B({pb}) @ {plies}-ply"), &r);
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

fn parse_player(s: &str) -> Player {
    if s.eq_ignore_ascii_case("b") || s.eq_ignore_ascii_case("black") {
        Player::Black
    } else {
        Player::White
    }
}

/// Parse "pos:count,pos:count,..." (in the player's own 1..24 path coords).
fn parse_side(spec: &str, player: Player, board: &mut Board) {
    for tok in spec.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if let Some((p, c)) = tok.split_once(':') {
            if let (Ok(pos), Ok(cnt)) = (p.trim().parse::<u8>(), c.trim().parse::<u8>()) {
                if (1..=24).contains(&pos) {
                    board.place(player, pos, cnt);
                }
            }
        }
    }
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

/// Index of the move maximising the net's equity at the given lookahead
/// (`extra_plies` beyond the resulting position; 0 = static 1-ply).
fn best_index(net: &Net, turns: &[Turn], mover: Player, extra_plies: u8) -> usize {
    let opp = mover.opponent();
    let mut bi = 0;
    let mut be = f32::NEG_INFINITY;
    for (i, t) in turns.iter().enumerate() {
        let e = -position_equity(net, &t.board, opp, extra_plies, Some(1));
        if e > be {
            be = e;
            bi = i;
        }
    }
    bi
}

/// Measure the engine's error rate (equity lost per decision) vs deep rollouts —
/// the standard backgammon strength metric. Lower = stronger; ~0 means the
/// engine plays at rollout level.
fn cmd_er(args: &[String]) -> ExitCode {
    let net = match load_net(&str_arg(args, "--in", "")) {
        Some(n) => n,
        None => {
            eprintln!("er: need a valid --in PATH");
            return ExitCode::FAILURE;
        }
    };
    let m = arg(args, "--positions", 300usize);
    let n = arg(args, "--trials", 120usize);
    let seed = arg(args, "--seed", 77u64);

    let t0 = std::time::Instant::now();
    println!("Building bear-off table (rollout truncation)...");
    let table = BearoffTable::build();

    // Collect M decision positions (>1 legal play) from net self-play.
    let mut rng = Rng::new(seed);
    let mut positions: Vec<(Board, Player, [u8; 2])> = Vec::with_capacity(m);
    'gen: while positions.len() < m {
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
            if turns.len() > 1 {
                positions.push((board.clone(), mover, dice));
                if positions.len() >= m {
                    break 'gen;
                }
            }
            let idx = best_index(&net, &turns, mover, 0);
            board = turns[idx].board.clone();
            mover = mover.opponent();
        }
    }

    // Evaluate ER in parallel over positions.
    let threads = default_threads();
    let chunk = positions.len().div_ceil(threads);
    let net = &net;
    let table = &table;
    let mut partials: Vec<(f64, f64, usize, usize, usize)> = Vec::new();
    std::thread::scope(|s| {
        let handles: Vec<_> = positions
            .chunks(chunk)
            .enumerate()
            .map(|(ci, slice)| {
                s.spawn(move || {
                    let mut rng = Rng::new(seed ^ (0xABCD ^ ci as u64).wrapping_mul(0x9E3779B1));
                    let (mut er1, mut er2) = (0.0f64, 0.0f64);
                    let (mut a1, mut a2, mut cnt) = (0usize, 0usize, 0usize);
                    for (board, mover, dice) in slice {
                        let opp = mover.opponent();
                        let turns = generate_turns_cfg(board, *mover, *dice, false, Some(1));
                        let mut eqs = Vec::with_capacity(turns.len());
                        for t in &turns {
                            let (mean, _) = rollout_equity(net, table, &t.board, opp, n, &mut rng);
                            eqs.push(-mean);
                        }
                        let (best_idx, best) = eqs.iter().enumerate().fold(
                            (0usize, f32::NEG_INFINITY),
                            |(bi, be), (i, &e)| if e > be { (i, e) } else { (bi, be) },
                        );
                        let i1 = best_index(net, &turns, *mover, 0);
                        let i2 = best_index(net, &turns, *mover, 1);
                        er1 += (best - eqs[i1]).max(0.0) as f64;
                        er2 += (best - eqs[i2]).max(0.0) as f64;
                        a1 += (i1 == best_idx) as usize;
                        a2 += (i2 == best_idx) as usize;
                        cnt += 1;
                    }
                    (er1, er2, a1, a2, cnt)
                })
            })
            .collect();
        for h in handles {
            partials.push(h.join().unwrap());
        }
    });

    let (er1, er2, a1, a2, cnt) = partials.into_iter().fold(
        (0.0, 0.0, 0, 0, 0),
        |(e1, e2, x1, x2, c), (p1, p2, q1, q2, k)| (e1 + p1, e2 + p2, x1 + q1, x2 + q2, c + k),
    );
    let c = cnt.max(1) as f64;
    println!(
        "\nError rate vs rollouts ({cnt} decisions, {n} trials each, {:.1}s):",
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
    ExitCode::SUCCESS
}

/// Parse a one-line position spec used by `relay`/`agree`:
///   `white=24:13,18:2 black=24:15 turn=W dice=3,1 [white-off=N] [black-off=N] [first]`
fn parse_pos_line(spec: &str) -> Option<(Board, Player, [u8; 2], bool)> {
    let mut board = Board::empty();
    let mut turn = Player::White;
    let mut dice: Option<[u8; 2]> = None;
    let mut first = false;
    for tok in spec.split_whitespace() {
        let (k, v) = tok.split_once('=').unwrap_or((tok, ""));
        match k {
            "white" => parse_side(v, Player::White, &mut board),
            "black" => parse_side(v, Player::Black, &mut board),
            "white-off" => board.off[Player::White.index()] = v.parse().unwrap_or(0),
            "black-off" => board.off[Player::Black.index()] = v.parse().unwrap_or(0),
            "turn" => turn = parse_player(v),
            "dice" => {
                let d: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if d.len() == 2 {
                    dice = Some([d[0], d[1]]);
                }
            }
            "first" => first = v.is_empty() || v.eq_ignore_ascii_case("true") || v == "1",
            _ => {}
        }
    }
    Some((board, turn, dice?, first))
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
        let Some((board, mover, dice, first)) = parse_pos_line(&line) else {
            eprintln!("  ? unparseable line (need at least dice=d1,d2)");
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
                    if let Some((board, mover, dice, first)) = parse_pos_line(prefix) {
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
    println!(
        "{label}: {} games — win rate {:.1}% ({}/{}) | {:+.3} ppg",
        r.games,
        r.win_rate() * 100.0,
        r.wins,
        r.games,
        r.ppg()
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
        Some("relay") => cmd_relay(&args[1..]),
        Some("agree") => cmd_agree(&args[1..]),
        _ => {
            eprintln!(
                "usage:\n  engine-train train [--games N] [--hidden H] [--lr L] [--explore E] [--seed S] [--threads T] [--sync K] [--init PATH] [--out PATH]\n  engine-train bench --in PATH [--games N] [--plies P] [--vs heuristic|random] [--hplies P]\n  engine-train duel --a PATH --b PATH [--games N] [--plies P]\n  engine-train analyze --net PATH --white \"24:13,18:2\" --black \"24:15\" --dice 3,1 --turn W [--plies P] [--head unlimited] [--white-off N] [--black-off N] [--top N]\n  engine-train er --in PATH [--positions M] [--trials N] [--seed S]\n  engine-train relay --net PATH [--ply P] [--top N]   (reads position lines from stdin → best plays)\n  engine-train agree --net PATH [--file decisions.txt] [--mat match.MAT] [--ply P]   (move-agreement % + ER vs LogasAI)"
            );
            ExitCode::FAILURE
        }
    }
}
