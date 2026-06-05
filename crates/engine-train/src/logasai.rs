//! Sparring + benchmarking tooling for measuring the engine against an external
//! reference (notably **LogasAI**, a strong Windows-only long-nardy engine with
//! no API — see `docs/logasai-benchmark.md`).
//!
//! Three reusable pieces, all driven by ONE small, documented decision format so
//! they work regardless of how the data was produced (typed by hand during a live
//! spar, or extracted from a `.MAT` export):
//!
//!   * [`rank`] — given a position + roll, the engine's best plays (the *relay*:
//!     you mirror our move in LogasAI and record the result).
//!   * [`agree`] — over a list of [`Decision`]s (a position + the move that was
//!     actually played), the **move-agreement %** and the **error rate (ER,
//!     equity lost per decision)** of the played moves vs our best — the standard
//!     world-wide backgammon strength metric.
//!   * [`replay_mat`] — reconstruct a game (a stream of [`Decision`]s) from a
//!     `.MAT` match transcript, so a whole LogasAI game can be benchmarked at once.
//!
//! Moves are matched to legal turns by their **collapsed endpoint segments**
//! (e.g. `24/18/16` reads as `24/16`), mirroring the web UI's notation — so the
//! sub-move ordering a `.MAT` may not preserve never causes a false mismatch.

use engine_core::board::Board;
use engine_core::moves::{generate_turns_cfg, Turn};
use engine_core::net::Net;
use engine_core::player::Player;
use engine_core::search::position_equity;

/// Standard FSNR head limit used throughout (one checker off the head per turn).
pub const HEAD_LIMIT: Option<u8> = Some(1);

/// Collapse a turn into LogasAI-style endpoint segments: each moved checker as
/// `(start, end)` where `end == 0` means borne off. Mirrors the web UI's
/// `fmtTurn` collapsing of single-checker chains (`24→18→16` reads as `24/16`),
/// so notation that drops the intermediate point still matches.
pub fn segments(t: &Turn) -> Vec<(u8, u8)> {
    let ms = &t.moves;
    let mut segs = Vec::new();
    let mut i = 0;
    while i < ms.len() {
        let start = ms[i].from;
        let mut j = i;
        while j + 1 < ms.len() && !ms[j].bear_off && ms[j + 1].from == ms[j].to {
            j += 1;
        }
        let end = if ms[j].bear_off { 0 } else { ms[j].to };
        segs.push((start, end));
        i = j + 1;
    }
    segs.sort_unstable();
    segs
}

/// Human-readable notation for a turn (`24/21 13/11`, `6/off`, or `pass`).
pub fn format_turn(t: &Turn) -> String {
    let segs = {
        // Render in play order (not sorted) for readability.
        let ms = &t.moves;
        let mut out = Vec::new();
        let mut i = 0;
        while i < ms.len() {
            let start = ms[i].from;
            let mut j = i;
            while j + 1 < ms.len() && !ms[j].bear_off && ms[j + 1].from == ms[j].to {
                j += 1;
            }
            out.push(if ms[j].bear_off {
                format!("{start}/off")
            } else {
                format!("{start}/{}", ms[j].to)
            });
            i = j + 1;
        }
        out
    };
    if segs.is_empty() {
        "pass".to_string()
    } else {
        segs.join(" ")
    }
}

/// Parse a played-move string like `24/21 13/11`, `6/off 5/3`, or `pass` into the
/// canonical sorted endpoint segments. Returns `None` if unparseable.
pub fn parse_play(s: &str) -> Option<Vec<(u8, u8)>> {
    let s = s.trim();
    if s.is_empty()
        || s.eq_ignore_ascii_case("pass")
        || s == "—"
        || s.eq_ignore_ascii_case("пропуск")
    {
        return Some(vec![]);
    }
    let mut segs = Vec::new();
    for tok in s.split_whitespace() {
        let (a, b) = tok.split_once('/')?;
        let from: u8 = a.trim().parse().ok()?;
        let to = if b.eq_ignore_ascii_case("off")
            || b == "0"
            || b.eq_ignore_ascii_case("выкид")
        {
            0
        } else {
            b.trim().parse().ok()?
        };
        segs.push((from, to));
    }
    segs.sort_unstable();
    Some(segs)
}

/// Index of the legal turn whose collapsed segments equal the played move.
pub fn match_index(turns: &[Turn], played: &str) -> Option<usize> {
    let target = parse_play(played)?;
    turns.iter().position(|t| segments(t) == target)
}

/// Index of the move maximising the net's equity at `extra_plies` lookahead
/// beyond the resulting position (0 = static 1-ply).
pub fn best_index(net: &Net, turns: &[Turn], mover: Player, extra_plies: u8) -> usize {
    let opp = mover.opponent();
    let mut bi = 0;
    let mut be = f32::NEG_INFINITY;
    for (i, t) in turns.iter().enumerate() {
        let e = -position_equity(net, &t.board, opp, extra_plies, HEAD_LIMIT);
        if e > be {
            be = e;
            bi = i;
        }
    }
    bi
}

/// A ranked play: notation, equity (mover's perspective), win probability.
pub struct Ranked {
    pub notation: String,
    pub equity: f32,
    pub win: f32,
}

/// Rank the legal plays for a position+roll by net equity at `plies` lookahead.
/// This is the *relay*: feed a LogasAI position and roll, get our best move(s).
pub fn rank(
    net: &Net,
    board: &Board,
    mover: Player,
    dice: [u8; 2],
    first: bool,
    plies: u8,
) -> Vec<Ranked> {
    let extra = plies.max(1) - 1;
    let opp = mover.opponent();
    let turns = generate_turns_cfg(board, mover, dice, first, HEAD_LIMIT);
    let mut out: Vec<Ranked> = turns
        .iter()
        .map(|t| Ranked {
            notation: format_turn(t),
            equity: -position_equity(net, &t.board, opp, extra, HEAD_LIMIT),
            win: 1.0 - net.evaluate_board(&t.board, opp).win,
        })
        .collect();
    out.sort_by(|a, b| b.equity.partial_cmp(&a.equity).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// One scored decision: a position + roll + the move that was actually played.
#[derive(Clone)]
pub struct Decision {
    pub board: Board,
    pub mover: Player,
    pub dice: [u8; 2],
    pub first: bool,
    pub played: String,
}

/// Aggregate agreement / error-rate result.
#[derive(Clone, Copy, Debug, Default)]
pub struct AgreeResult {
    pub decisions: usize, // positions with a real choice (>1 legal turn)
    pub matched: usize,   // played move found among the legal turns
    pub agreements: usize, // played move == our best
    pub unmatched: usize, // played move could not be matched (parse / illegal)
    pub er_sum: f64,      // summed equity lost vs our best, over matched decisions
}
impl AgreeResult {
    pub fn agreement_rate(&self) -> f64 {
        if self.matched == 0 {
            0.0
        } else {
            self.agreements as f64 / self.matched as f64
        }
    }
    pub fn error_rate(&self) -> f64 {
        if self.matched == 0 {
            0.0
        } else {
            self.er_sum / self.matched as f64
        }
    }
}

/// Move-agreement % and ER of the played moves vs our `plies`-ply best, over a
/// list of decisions. Positions with a single legal turn are skipped (no choice).
pub fn agree(net: &Net, decisions: &[Decision], plies: u8) -> AgreeResult {
    let extra = plies.max(1) - 1;
    let mut r = AgreeResult::default();
    for d in decisions {
        let turns = generate_turns_cfg(&d.board, d.mover, d.dice, d.first, HEAD_LIMIT);
        if turns.len() <= 1 {
            continue; // forced — not a decision
        }
        r.decisions += 1;
        let bi = best_index(net, &turns, d.mover, extra);
        match match_index(&turns, &d.played) {
            Some(pi) => {
                r.matched += 1;
                if pi == bi {
                    r.agreements += 1;
                }
                let opp = d.mover.opponent();
                let best_eq = -position_equity(net, &turns[bi].board, opp, extra, HEAD_LIMIT);
                let play_eq = -position_equity(net, &turns[pi].board, opp, extra, HEAD_LIMIT);
                r.er_sum += (best_eq - play_eq).max(0.0) as f64;
            }
            None => r.unmatched += 1,
        }
    }
    r
}

// ---- .MAT transcript replay ------------------------------------------------

/// Reconstruct the decisions of a single game from an ordered list of plies —
/// `(dice, move_notation)` in play order (White, Black, White, …) starting from
/// the standard opening. Each entry yields a [`Decision`] (the position BEFORE
/// the move) and advances the board by matching the move to a legal turn.
///
/// Returns the decisions plus the index of the first ply that could not be
/// reconstructed (a parse/illegal/notation mismatch), if any — so callers can
/// report partial games instead of silently dropping them.
pub fn replay_plies(plies: &[([u8; 2], String)]) -> (Vec<Decision>, Option<usize>) {
    let mut board = Board::starting();
    let mut mover = Player::White;
    let mut first = [true, true];
    let mut out = Vec::with_capacity(plies.len());
    for (k, (dice, mv)) in plies.iter().enumerate() {
        let turns = generate_turns_cfg(&board, mover, *dice, first[mover.index()], HEAD_LIMIT);
        let idx = match match_index(&turns, mv) {
            Some(i) => i,
            None => return (out, Some(k)),
        };
        out.push(Decision {
            board: board.clone(),
            mover,
            dice: *dice,
            first: first[mover.index()],
            played: mv.clone(),
        });
        board = turns[idx].board.clone();
        first[mover.index()] = false;
        mover = mover.opponent();
    }
    (out, None)
}

/// Parse the move plies out of a `.MAT` match transcript (best-effort, standard
/// Jellyfish/GNU move grammar: lines like `  3) 31: 24/21 13/11   62: 13/7 24/18`).
/// The left half is the first mover's play, the right the second's. Dice are the
/// two digits before the colon (`31` → `[3,1]`). Lines without a `N)` prefix
/// (headers, scores, blank) are ignored.
///
/// NB: the exact dialect LogasAI writes for длинные нарды is unverified against a
/// real export (see `docs/logasai-benchmark.md`); this parses the *standard* form
/// and is covered by a round-trip test. Adjust here once a real `.MAT` is in hand.
pub fn parse_mat(text: &str) -> Vec<([u8; 2], String)> {
    let mut plies = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        // a move line starts with "<n>)"
        let Some(rest) = line
            .split_once(')')
            .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            .map(|(_, r)| r.trim())
        else {
            continue;
        };
        // split the two halves on whitespace runs of >=2 spaces or a tab; fall
        // back to detecting the second "DD:" token.
        for half in split_halves(rest) {
            if let Some(p) = parse_half(&half) {
                plies.push(p);
            }
        }
    }
    plies
}

/// Split a move line's body into its (up to two) "DICE: moves" halves.
fn split_halves(rest: &str) -> Vec<String> {
    // Find token boundaries that look like a dice header "DD:".
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let mut halves = Vec::new();
    let mut cur = String::new();
    for tok in toks {
        let is_header = tok.len() >= 3
            && tok.ends_with(':')
            && tok[..tok.len() - 1].chars().all(|c| c.is_ascii_digit());
        if is_header && !cur.is_empty() {
            halves.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(tok);
    }
    if !cur.is_empty() {
        halves.push(cur);
    }
    halves
}

/// Parse one `DD: moves` half into `([d1,d2], "moves")`.
fn parse_half(half: &str) -> Option<([u8; 2], String)> {
    let (head, moves) = half.split_once(':')?;
    let head = head.trim();
    if head.len() < 2 || !head.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let d1 = head[..1].parse().ok()?;
    let d2 = head[1..2].parse().ok()?;
    let moves = moves.trim();
    // strip trailing annotations LogasAI may add (e.g. "(пропуск)") — keep tokens
    // that contain '/', else treat as pass.
    let mv: Vec<&str> = moves.split_whitespace().filter(|t| t.contains('/')).collect();
    Some(([d1, d2], if mv.is_empty() { "pass".into() } else { mv.join(" ") }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::net::Net;

    const NET_BYTES: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../models/nardy-net.bin"));

    #[test]
    fn parse_play_roundtrip() {
        assert_eq!(parse_play("pass"), Some(vec![]));
        assert_eq!(parse_play("24/21 13/11"), Some(vec![(13, 11), (24, 21)]));
        assert_eq!(parse_play("6/off"), Some(vec![(6, 0)]));
        assert_eq!(parse_play("garbage"), None);
    }

    #[test]
    fn match_and_best_are_consistent() {
        // Every legal turn from the opening must match its own formatted notation,
        // and best_index must point at a real legal turn.
        let net = Net::from_bytes(NET_BYTES).unwrap();
        let board = Board::starting();
        let turns = generate_turns_cfg(&board, Player::White, [3, 1], true, HEAD_LIMIT);
        assert!(!turns.is_empty());
        for t in &turns {
            let note = format_turn(t);
            assert_eq!(
                match_index(&turns, &note),
                turns.iter().position(|x| segments(x) == segments(t)),
                "turn {note} should match itself",
            );
        }
        let bi = best_index(&net, &turns, Player::White, 0);
        assert!(bi < turns.len());
    }

    #[test]
    fn agree_with_self_is_perfect() {
        // If the "played" move IS our best move, agreement is 100% and ER is 0.
        // Use a spread mid-game position (NOT the opening, which is a single forced
        // turn) so there is a real choice among many legal turns.
        let net = Net::from_bytes(NET_BYTES).unwrap();
        let mut board = Board::empty();
        board.place(Player::White, 24, 13);
        board.place(Player::White, 13, 1);
        board.place(Player::White, 7, 1);
        board.place(Player::Black, 24, 15);
        let dice = [3, 1];
        let turns = generate_turns_cfg(&board, Player::White, dice, false, HEAD_LIMIT);
        assert!(turns.len() > 1, "need a real choice, got {} turns", turns.len());
        let bi = best_index(&net, &turns, Player::White, 0);
        let played = format_turn(&turns[bi]);
        let dec = Decision { board, mover: Player::White, dice, first: false, played };
        let r = agree(&net, &[dec], 1);
        assert_eq!(r.matched, 1, "our own best move must match a legal turn");
        assert_eq!(r.agreements, 1, "playing our best move agrees with our best");
        assert!(r.error_rate() < 1e-4, "ER of our own best move must be ~0");
    }

    #[test]
    fn replay_reconstructs_a_self_consistent_game() {
        // Build a short legal sequence by always playing the first legal turn, then
        // hand its notation back through the replay path and confirm every ply
        // reconstructs (the matcher inverts the generator).
        let mut board = Board::starting();
        let mut mover = Player::White;
        let mut first = [true, true];
        let mut plies = Vec::new();
        let dice_seq = [[3, 1], [6, 4], [5, 2], [6, 6], [2, 1], [4, 3]];
        for d in dice_seq {
            let turns = generate_turns_cfg(&board, mover, d, first[mover.index()], HEAD_LIMIT);
            let t = &turns[0];
            plies.push((d, format_turn(t)));
            board = t.board.clone();
            first[mover.index()] = false;
            mover = mover.opponent();
        }
        let (decisions, fail) = replay_plies(&plies);
        assert_eq!(fail, None, "every ply should reconstruct");
        assert_eq!(decisions.len(), plies.len());
    }

    #[test]
    fn parse_mat_extracts_dice_and_moves() {
        // A standard-format move line with two halves.
        let mat = "  1) 31: 24/20   64: 24/18 13/9\n  2) 11: 23/22 23/22\n";
        let plies = parse_mat(mat);
        assert_eq!(plies.len(), 3);
        assert_eq!(plies[0], ([3, 1], "24/20".to_string()));
        assert_eq!(plies[1], ([6, 4], "24/18 13/9".to_string()));
        assert_eq!(plies[2].0, [1, 1]);
    }
}
