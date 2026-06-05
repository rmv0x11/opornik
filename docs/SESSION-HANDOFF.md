# Session handoff — opornik (читать первым после очистки контекста)

Живой сайт: https://rmv0x11.github.io/opornik/ · деплой автоматический (правило `auto-deploy-no-ask`).
Версионирование: `web/package.json` → `__APP_VERSION__` (подвал меню) + `CHANGELOG.md` + версия в коммите ветки `gh-pages`. **В проде сейчас — v0.9.10** (`index-DWu-jilO.js`, gh-pages коммит `304f5dc`).
v0.9.10: «обзор лучших ходов» («Оценка») теперь всегда на виду — `analyze()` вызывается автоматически при входе в фазу `humanMove` (`enterHumanMove`/`resumeFrom`/`replayFrom` → `void analyze()`), кнопка «Оценка» убрана. (Пользователь уточнил через AskUserQuestion: имел в виду именно «Оценку» текущего хода, хотел «всегда на виду».) e2e: тесты «inline analysis auto-shows…» больше не кликают кнопку.
v0.9.9: откатил агрессивный fit-сквиз листа партии (0.9.6–0.9.8 ужимали лог в ~1–2 строки ради no-page-scroll → обзор ходов был тесным). Теперь лог снова просторный прокручиваемый блок (`.log{max-height:clamp(200px,46dvh,340px)}` на мобиле), страница прокручивается по необходимости. `.fit` оставлен ТОЛЬКО для высоты доски (старт высокая, в партии чуть ниже: `.play.fit :global(.board){--row-h:...24rem...}`); никакого `.play.fit{height/flex}` и flex-сквиза лога больше нет. Урок: НЕ ужимать лист партии/обзор — пользователь ценит просторный обзор > отсутствие прокрутки страницы.
v0.9.8: фикс загрузки WASM на iPhone/Safari. `WebAssembly.instantiateStreaming` в WebKit (особенно в Web Worker) кидает «Unexpected response MIME type. Expected 'application/wasm'» даже при правильном Content-Type → fallback wasm-bindgen не срабатывает (MIME правильный) → краш на айфоне. Фикс в `web/src/engine/worker.ts`: грузим wasm байтами (`fetch(wasmUrl).then(r=>r.arrayBuffer())`) и `init({module_or_path: bytes})` → идёт через `WebAssembly.instantiate` (без проверки MIME). Playwright WebKit 26.4 баг НЕ воспроизводит (грузит и через streaming), но фикс обходит проблемный путь целиком; проверено: WebKit+Chromium грузят движок без ошибок (preview и live).
v0.9.7: починена прокрутка листа партии на телефоне. УРОК: `<details>` с `display:flex` оборачивает контент в анонимный бокс → `flex` на внутреннем списке НЕ работает (список не сжимался/не скроллился, обрезался). Лист партии переписан с `<details>/<summary>` на `<div class="gamelog">` + кнопка `.gl-summary` со `$state logOpen` → flex/скролл работает надёжно. Цепочка fit-скролла: `.play.fit{height:calc(100dvh-3.2rem);flex col}` → `.panel{flex:1;min-height:0}` → `.gamelog{flex:1;min-height:7rem;overflow:hidden}` → `.log{flex:1 1 0; min-height:0; overflow-y:auto}` (ВАЖНО `flex-basis:0`, не auto — иначе список держит высоту контента и не скроллится). Тест-хелпер `makeHumanMove` (play.spec) теперь ретраит, а не бросает ход. Новые e2e (мобильный вьюпорт 390×844): «mobile: game-log usability» — лист скроллится внутри, страница нет, сворачивание работает.
v0.9.5→0.9.6 (адаптив телефона, `Board.svelte` `--row-h` + `Play.svelte` `@media max-width:600px`): на стартовом экране доска высокая (заполняет экран); как только в партии есть ходы (`history.length>0` → класс `.fit` на `.play`) — `.play.fit { height: calc(100dvh - 3.2rem); display:flex; column }`, доска ниже (`--row-h` override), а `.gamelog` `flex:1`+внутренний скролл забирает остаток → вся раскладка в один экран без прокрутки страницы. Десктоп не тронут (правила в `@media max-width:600px`). Проверено 412×870 и 360×640, overflow=0.

## ⚠️ Инцидент: внешний откат `moves.rs`
В этой сессии `crates/engine-core/src/moves.rs` был молча **откатан к HEAD** (системная заметка «modified by a linter») — потерялись и мои правки, и доки/тесты прошлой сессии. Почти наверняка это **автосейв устаревшего буфера в редакторе пользователя** (откатило ТОЛЬКО открытый файл; остальные крейты целы). Восстановлено и перепроверено. **Если правки в `moves.rs` снова исчезнут** — переприменить рефактор (см. ниже) и тесты; логика правил при этом в HEAD корректна (идентична задеплоенной). Рабочее дерево НЕ закоммичено (вся работа сессий — uncommitted на `main`); деплой идёт через готовый `web/dist` в `gh-pages`, поэтому прод защищён от откатов исходников. При желании защитить исходники — предложить пользователю коммит в отдельную ветку (на `main` без спроса не коммитить).

## Сделано в v0.9.4 (задеплоено)
1. **Передвижение фишек — корректный разбор всех порядков.** Движок:
   `moves.rs` рефактор `finalize`→`select_maximal`+`maximal_leaves`+`dedup_to_turns`;
   новая `pub fn legal_sequences(...)` (все легальные УПОРЯДОЧЕННЫЕ
   последовательности под-ходов, каждая легальна на каждом шаге; транспозиции НЕ
   схлопываются). `GameState::legal_sequences()` зеркалит `legal_turns()`.
   WASM: `legalSequences()` (lib.rs) → JSON `SequenceDto[] {moves, turn_id}` через
   `dto::sequence_dtos`. UI (`Play.svelte`): мультимножество заменено на
   ПРЕФИКС-сопоставление по последовательностям (`sameMove`/`isPrefix`/
   `availableHops`/`completeSeq`), коммит через `play(completeSeq.turn_id)`.
   Тесты: 73 в engine-core (вкл. 5 sequence-тестов + 600-позиционный property-тест),
   e2e «move-building: a checker can play EITHER die first…».
2. **Розыгрыш первого хода (ФСНР) — без повторного броска.** `doOpeningRoll`
   (Play.svelte): каждый бросает одну кость; старший ходит первым и играет первый ход
   ЭТИМИ ЖЕ двумя костями (по одной от игрока); повторного броска НЕТ; при равенстве —
   переброс. human-first → сразу `enterHumanMove(you,opp)` (БЕЗ кнопки «Бросить
   кости»); ai-first → `aiRoll([you,opp])` (без второго броска). Второй игрок дальше
   бросает свои две кости (его первый ход → исключение головы 6-6/4-4/3-3 ещё работает;
   опенинг-кости всегда разные, так что на первом ходу исключение не срабатывает —
   движок не трогали, всё на гейтинге `dice[0]==dice[1]`). `aiRoll(preset?)` добавлен.
3. **Доска (`Board.svelte`):** убрана центральная ГОРИЗОНТАЛЬНАЯ полоса (`.midbar`
   удалён); ряды в `.table` сходятся напрямую (делитель — вертикальный `.bar`). Кости
   (бросок/после броска), лоток сброса (`.off-chip.white/.black`) и «дом» — в боковой
   панели `.rail` справа. `--pt: clamp(18px, calc((100vw - 3.5rem - var(--rail))/13),
   50px)`, `--rail: clamp(44px,11vw,68px)`. Цель glide-выкида прежняя
   (`.board .off-chip.{color}`, теперь в `.rail`).

## НОВОЕ для тестов (опенинг изменился!)
e2e больше НЕ ждут «Бросить кости» сразу после опенинга. Хелперы в `play.spec.ts`:
`waitOpeningReady(page)` (ждёт source ИЛИ «Бросить кости»), `reachHumanRoll(page)`
(доводит до фазы броска человека), `startAndRoll` (ролл только если кнопка есть).
preview (`tests/preview/load.spec.ts`) и live (`tests/live/smoke.spec.ts`,
`ai-notation.spec.ts`) тоже сделаны opening-aware; live `makeHumanMove` ретраит, а не
бросает ход при временно скрытом «Подтвердить».

## Осталось до 1.0
Опц.: **звук**, **PWA**. (Правила, ничья в Classic, эндпоинт легальных под-ходов,
опенинг-ролл — сделаны.)

## Жёсткие ограничения (не сломать)
- e2e-селекторы/тексты: `.board`, `.board .point(.source/.dest/.selected/.last)`,
  `[data-cell]`, `.board .checker`, `.board .rollzone`, `.board .bar`,
  `.board .off-chip.{white,black}`, `.winbar .label`, `.gamelog .logrow .mv/.ev`,
  `.ranked li.best`, `.ailast .minidie`, `.resign summary`, `.import`, `.save-go`,
  `.open-res`/`.opening`, `.ptnum` (число пункта). Тексты кнопок: «Начать
  партию/матч», «Разыграть первый ход», «Бросить кости», «Подтвердить ход»,
  «Отменить», «Выкинуть шашку», «Оценка», «Сдаться — оин/Сдаться с марсом»,
  «Удвоить», «Тайк/Пас/Бивер», «Продолжить с этой позиции», «Переиграть этот ход»,
  «Понятно, играть». Меняешь разметку → синхронно правь spec'и.
- Координаты: `web/src/lib/board/coords.ts` (`phys`/`posOfPhys`).

## Состояние движка (Rust)
- `setTurn(color)`, `set_dice`, `legalTurns`, **`legalSequences`**, `applyTurn(id)`,
  `bestMove`, `analyze`, `cubeDecision`, `offerDouble`, `beaver`,
  `setPosition(+turn_number)`, `reset`, `setCrawford`.
- Expert 3-ply ограничен: forward-pruning `SEARCH_WIDTH=8`/`ROOT_WIDTH=12` +
  leaf-budget `PLAY_LEAF_BUDGET=90_000` → ~0.8с худший случай. ply≤2 — точно.
- Куб — Janowski. Ничья в Classic — `Outcome::Draw` + `first_mover`/`equalizer_for`.
- Известное ограничение: сеть OOD для хачапури.

## Процесс (проверка → деплой) — всё прошло в v0.9.4
1. `cd /Users/rmv/personal/opornik/web && npm run check` (ВАЖНО: cargo/wasm-pack
   роняют cwd в корень репо — для npm/playwright всегда `cd .../web`).
2. Менял Rust? → `cargo test -p engine-core` (73); пересобрать WASM:
   `wasm-pack build crates/engine-wasm --target web --out-dir ../../web/src/engine/pkg`.
3. `cd web && npx playwright test` (25 тестов; длинные могут мигать — перезапуск).
4. `npm run build` + `npx playwright test --config=playwright.preview.config.ts`.
5. Бамп версии `web/package.json` + `CHANGELOG.md`.
6. gh-pages: `touch web/dist/.nojekyll`; `git worktree add /tmp/opornik-ghp gh-pages`;
   очистить (кроме `.git`); `cp -R web/dist/. .`; commit `-c
   user.name="Maxim Rudenko" -c user.email="rmv0x11@gmail.com"` с версией; push;
   `git worktree remove --force`. Опрос live на новый хэш бандла.
7. Live: `cd web && npx playwright test --config=playwright.live.config.ts`.

## Процессный урок
Параллельные ПИШУЩИЕ workflow-агенты в одном дереве затирают правки — для пишущих
агентов `isolation: 'worktree'` или read-only (в этой сессии верификация была
read-only Explore-агентами — норм). Workflow-агент может ошибиться на устаревшем
чтении файла (probe:movement выдал ложный «blocker» про отсутствующую функцию — но
файл реально откатил внешний редактор; всегда перепроверять находки агентов фактами:
`cargo test`/grep).
