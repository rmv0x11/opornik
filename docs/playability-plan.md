# План играбельности (Фаза 2)

Синтез design-workflow (4 агента: WASM-API, доска, app/relay UX, сборка). Полный
разбор — в транскрипте workflow; здесь — согласованные решения и порядок работ.

## Согласованные решения
- **engine-wasm** — новый крейт `crates/engine-wasm` (`cdylib`+`rlib`), тонкая
  `wasm-bindgen`-обёртка над `engine-core`. Веса сети **вшиты** через
  `include_bytes!("../../models/nardy-net.bin")` (≈64 КБ) + запасной `with_net_bytes`.
- **Граница JS↔WASM** — JSON-строки через `serde_json` (без `serde-wasm-bindgen`).
  Stateful `Engine` хранит `GameState` + `Net` + ленивую `BearoffTable`.
  Кости бросает JS и передаёт через `set_dice` (engine-core остаётся без RNG).
- **Поиск/оценка**: best_move/equity/analyze(ply≥2) через `RaceOrNet` (сеть +
  точная концовка из bearoff). winProbabilities/cubeDecision/analyze(ply=1) — по сети.
- **Фронт** — Svelte 5 (runes) + Vite + `svelte-spa-router`, один Web Worker с
  движком. Доска — Chessground-стиль (HTML/CSS-трансформы + SVG-оверлей), без Canvas.
- Координаты в UI = path-позиции `1..24` каждого игрока (24=голова, 1..6=дом, 0=сброс).
  Одна таблица `slot→phys` на ориентацию — единственное место, где учитывается сторона.

## Порядок работ (вертикальный срез → затем полнота)
1. ✅ Дизайн (workflow).
2. `engine-wasm`: `Cargo.toml`, `src/dto.rs`, `src/lib.rs` (Engine API). ← пишу сейчас
3. Поставить инструменты: `cargo install wasm-pack`, `pnpm` (после ER, чтобы не
   тормозить точный замер).
4. `wasm-pack build crates/engine-wasm --target web` → проверить сборку, починить.
5. Минимальный Vite+Svelte app: worker + client (RPC), стартовая доска (Board+coords),
   **играбельный цикл против ИИ** (бросок → выбор хода → ИИ-ответ) — вертикальный срез.
6. Достроить по дизайну: win%-бар, подсказки/стрелки, анализ-редактор, relay,
   разбор партии, варианты/куб/матч-UI.

## Карта файлов (целевая, из дизайна)
```
crates/engine-wasm/{Cargo.toml, src/lib.rs, src/dto.rs}
web/
  package.json, vite.config.ts, index.html, src/main.ts, src/App.svelte
  src/engine/{worker.ts, client.ts, types.ts, pkg/(wasm-pack output, gitignore)}
  src/lib/board/{Board.svelte, coords.ts, types.ts, pieces.ts, overlay.ts,
                 Dice.svelte, Cube.svelte, board.css, index.ts}
  src/lib/{rules.ts, notation.ts, stores/*.svelte.ts}
  src/lib/components/{Nav, EngineLoader, VariantPicker, DifficultyPicker,
                      GamePanel, DiceTray, TurnControls, HintButton, WinBar,
                      CubeWidget, RankedMoves, EditorPalette, ... }.svelte
  src/routes/{Home, PlaySetup, PlayGame, Analysis, Relay, Review}.svelte
  public/models/nardy-net.bin   # если решим грузить fetch'ем вместо вшивания
```

## Engine API (JS-facing, JSON-строки)
`new Engine(variant)` · `getPosition()` · `setDice(d1,d2)` · `legalTurns()` ·
`applyTurn(id)` · `bestMove(ply)` · `analyze(ply)` · `winProbabilities()` ·
`equity(ply)` · `cubeDecision()` · `offerDouble()` · `beaver()` ·
`setPosition(setupJson)` · `reset()`. DTO: PositionDto/PointDto/TurnDto/
RankedTurnDto/ProbabilitiesDto/BestMoveDto/CubeDecisionDto/SetupDto.

## Установить (после ER)
`cargo install wasm-pack` · `corepack enable && corepack prepare pnpm@latest` (или `npm`).
Сборка: `wasm-pack build crates/engine-wasm --target web --out-dir pkg`.
