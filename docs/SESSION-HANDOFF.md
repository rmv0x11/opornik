# Session handoff — opornik (читать первым после очистки контекста)

## ⏸ ПАУЗА 2026-06-10 — ЭТАП 0 ДВИЖКА V2 (продолжить отсюда)
Пользователь сказал «поехали» по `docs/engine-v2-plan.md` (читать его первым). Этап 0 (тулинг) РЕАЛИЗОВАН и закоммичен (`41e0fbc`): `engine-train dataset` (роллаут-метки TSV), `er --per-phase --save/--load` (фиксированный eval-set, round-trip точный), `diag2` (диагностика «2-ply с роллаут-листьями»), Wilson CI + p-value в duel/bench, фазовый классификатор `phase_of`. 10/10 тестов, смоук пройден.
**На момент паузы в фоне крутились (проверить результаты!):**
1. `/tmp/opornik-stage0.log` — diag2 (60 решений, truth 96, leaf 24) + затем генерация `data/evalset-v1.tsv` (200×120). Если лог оборван/процесса нет — перезапустить: `./target/release/engine-train diag2 --net models/nardy-net.bin --positions 60 --trials 96 --leaf-trials 24 --seed 99` и `./target/release/engine-train er --in models/nardy-net.bin --positions 200 --trials 120 --seed 77 --per-phase --save data/evalset-v1.tsv` (суммарно ~45 мин). `data/evalset-v1.tsv` потом закоммитить.
2. Adversarial-ревью кода этапа 0 НЕ завершилось (workflow погиб с сессией) — код НЕ ревьюён; перед доверием числам diag2/eval-set прогнать ревью заново (фокус: перспективы/знаки в `rollout_probs` и diag2-листьях, round-trip `format_pos_line`↔`parse_pos_line`, `turn_key`, ci95/p-value).
**Дальше по плану:** интерпретировать diag2 (роллаут-листья чинят 2-ply ⇒ ошибка сети систематическая ⇒ рецепт подтверждён) → вписать в `docs/training-log.md` → этап 1 плана (кодировка v2: фичи прайма/тайминга, contact/race split, 4-softmax, multi-layer net.rs + экспорт из PyTorch). Замеренный темп: ~3 ч на 10k позиций × 432 роллаута (датасет этапа 2 = ночной прогон).

Живой сайт: https://rmv0x11.github.io/opornik/ · деплой автоматический (правило `auto-deploy-no-ask`).
Версионирование: `web/package.json` → `__APP_VERSION__` (подвал меню) + `CHANGELOG.md` + версия в коммите ветки `gh-pages`. **В проде сейчас — v0.9.20** (`index-BFDk_k-8.js`, gh-pages коммит `972dc87`; live-тесты 2/2 зелёные).

## Сессия v0.9.20 (2026-06-10, задеплоено; исходники закоммичены `740885c` в `feature/single-player-v0.9.10` и запушены)
- **«Разбор партии»** (`.postgame`, рендер при `phase==='over' && reviewStats`): структурированный `gameResult` (`{winnerIsHuman: bool|null, points}`, ставится в `finishGame` + draw-ветке `announceBoardWin`, чистится в `nextGame` И `rewindTo`); счётчики «✓ лучших/неточностей/ошибок» по `scoredMoves` (= `history.filter(isHuman && !cube && loss != null)`; у ходов ИИ `loss` захардкожен 0 — их НЕ оцениваем); точность = средняя потеря экв./ход + грейд (`gradeLabel`); спарклайн SVG (`sparkPoints`, win человека по ходам: `h.isHuman ? h.win : 1-h.win`, замкнут результатом); «главные потери» топ-3 (`worstMoves`, loss≥0.02) — клик `openReview(idx)` → `logOpen=true; reviewIdx=idx` + scrollIntoView строки. Классы только `.postgame/.pg-*` (НЕ переиспользовать `.review/.ranked/.logrow/.status` — strict-mode у тестов).
- **ФИКС двойного счёта (major из adversarial-ревью):** `rewindTo` при `phase==='over' && gameResult` ОТКАТЫВАЕТ начисленные очки (`matchScore[x] -= points`, `crawfordPlayed=false` если игра была кроуфордской) и всегда чистит `gameResult` — «Переиграть этот ход» из законченной партии больше не задваивает счёт (раньше это был пре-существующий баг, панель его приглашала).
- **Звук** (`web/src/lib/sound.ts`, синтез WebAudio, БЕЗ аудиофайлов, всё fail-silent — preview/live тесты требуют 0 console errors): `sfx.dice/move/bear/win/lose`; тогл «🔊 звук» в `.pips` (класс `.auto-toggle`, pref `opornik.sound`, по умолчанию ON). Хуки: `recordRoll` (НЕ opening — звук опенинга в `doOpeningRoll` в момент показа костей), `applyChain` (анимированные хопы по одному; drag-drop без анимации = ОДИН звук на жест), `animateTurn`, `finishGame` (+`silent` параметр: `announceBoardWin(pos, true)` из `resumeFrom`/`onMount` — восстановленная партия не играет джингл).
- e2e: 2 новых теста «post-game review …» (панель+статы+переход в разбор; откат очков при переигровке) — всего 32. `makeHumanMove` → resign — самый быстрый путь к game-over в тестах.
- Процесс: adversarial-review-workflow (4 измерения × скептики-верификаторы) подтвердил 5 уникальных проблем до деплоя — все исправлены до пуша.

## Сессия v0.9.11→v0.9.19 (всё задеплоено; исходники закоммичены в ветку `feature/single-player-v0.9.10`, кроме untracked `.github/workflows/deploy.yml` — GitHub отклоняет пуш файлов воркфлоу без scope `workflow`, лежит в дереве, коммить сам)
- **v0.9.12** сворачиваемые шкала шансов + обзор ходов (тоглы запоминаются в `localStorage`); пипсы переехали к доске в `.pips` (live-смоук теперь проверяет `.pips`, не `.meta`).
- **v0.9.13** «⚡ авто-бросок» (тогл в `.pips`); **v0.9.18** задержка авто-броска 700→350 мс.
- **v0.9.14** сдача: одна кнопка `.resign-btn` («🏳 Сдаться») → форма `.resign-confirm` («Да, сдаться»/«Отмена»); `<details>`/`.resign summary` убраны. **v0.9.15** кнопка сдачи переехала на строку статуса (`.status-row`) и перекрашена. **v0.9.16** фикс: `canResignSingle` читает ОТОБРАЖАЕМУЮ позицию (`displayPos ?? pos`) — собранный, но не подтверждённый выкид считается → оин, а не марс.
- **v0.9.17** честные кости: `rollDie` = `crypto.getRandomValues` + rejection-sampling (байты ≥252 отбрасываются) — строго 1/6, без modulo-bias. Панель «🎲 Кости — статистика». **v0.9.18** вкладки панели: Грани (честность), Куши (счёт всех 21 комбинации, копится в `localStorage` ключ `opornik.kushStats`), Последовательность (`rollSeq`, сессионная, история бросков вы/движок). Faces копятся в `opornik.diceStats`. `recordRoll()` вызывается в humanRoll/aiRoll(!preset)/doOpeningRoll.
- **v0.9.19** КУБ НА МАТЧ-ПОЙНТЕ: игрок в 1 очке от матча (`humanOneAway`/`aiOneAway` = `matchScore[x]===matchLength-1`) больше НЕ удваивает (куб бесполезен) — добавлено в `canHumanDouble` и в гейт двойного у движка (`aiTurn`); пометка `.crawford.post`. Сама игра Кроуфорда (`pos.crawford`) и так была корректна. e2e: «match: a player 1-away … cannot double».

## Движок + LogasAI (основная задача — продолжаем позже)
- **Тулинг готов** (`engine-train relay`/`agree`/`.MAT`, `crates/engine-train/src/logasai.rs`, тесты) — см. `docs/logasai-benchmark.md §5`. Способ мерить нас против LogasAI, когда будут его партии.
- **n-ply self-play обучение добавлено** (`--selfplay-plies`, `engine-train/src/lib.rs` `self_play_episode(.., plies)`; 1 = старый TD(0)).
- **Базовый ER чемпиона** (`models/nardy-net.bin`): 1-ply **0.0739** / 2-ply **0.0695** (150×80 роллаутов).
- **Эксперимент 2-ply self-play fine-tune — НУЛЕВОЙ.** Дуэль 50.5%/47.0%; ER 0.088/0.087 (хуже).
- **Эксперимент 2-ply ЦЕЛИ (`--target-plies 2`, 2026-06-10) — ТОЖЕ НУЛЕВОЙ.** Дуэль vs чемпион 47.8% (1-ply, 400) / 47.0% (2-ply, 200); ER 0.1042/0.0926 — хуже. Чемпион оставлен; сеть-архив `models/nardy-net-2plytgt.bin`; детали в `docs/training-log.md`. **Два нуля подряд ⇒ лимитирует кодировка/алгоритм, не TD-вариации.**
- **ПЛАН ДВИЖКА V2 ГОТОВ — `docs/engine-v2-plan.md` (читать перед любой работой над силой).** Итог deep-research (16 верифицированных источников) + инвентаризации кода: плато объясняется кодировкой без стратегических понятий (Fevga-прецедент: raw+TD не выучивает праймы в безударных нардах) и TD(0)-целями. Рецепт: wildbg-петля (supervised на роллаут-целях, поколениями), contact/race фазовые сети, raw+20-30 прайм/тайминг-фич, 4-softmax, PyTorch→экспорт, top-4 root pruning + SIMD; Fevga-3 reward shaping как запасной ход; AlphaZero/policy НЕ нужны (все топ-движки value+expectimax). Этап 0 = тулинг: dataset-генератор из роллаутов (er выбрасывает готовые метки!), стратифицированный eval-set, per-phase ER, CI на дуэлях, диагностика «роллаут-листья на 2-ply».
- ⚠️ Обучение: НЕ делать `renice +N` процессу обучения — macOS под нагрузкой зажимает nice+15 до 1 ядра (потеряли ~50 мин). Запускать на nice 0; 2-ply self-play ≈ ~1 партия/с на 9 ядрах (3600 партий ≈ ~60 мин).

v0.9.11: **кнопки действий — по центру доски.** Кубик «бросок» переехал из рейки в
центр поля (`Board.svelte` → новый click-through оверлей `.board-actions` внутри
`.table`, `position:relative`; `pointer-events:none` на контейнере, `auto` на кнопках
— построение хода не перекрывается). Roll-кнопка теперь ЕДИНАЯ: `.board .rollzone`
с `aria-label="Бросить кости"` (удовлетворяет И CSS-селектор `.board .rollzone`, И
`getByRole(/Бросить кости/)` — раньше это были две разные кнопки). Play передаёт
центральные кнопки через snippet-проп `center={boardCenter}`: «Разыграть первый ход»,
«⬆ Удвоить», «Тайк/Пас/Бивер», «✓ Подтвердить ход» (последняя — ТОЛЬКО когда ход
полностью собран `completeSeq`, чтобы не закрывать пункты при построении). «Отменить»
и «Выкинуть шашку» остались в нижней панели. Дубли кнопок из панели убраны (иначе
strict-mode у getByRole ловил бы 2 совпадения). Значения костей после броска —
по-прежнему в рейке (`Board` `.dice-slot` показывает `position.dice`).
**Лист партии:** действия с кубом теперь пишутся в `history` (`logCube`,
`LogEntry.cube`, строки 🎲² в логе); кнопка «📄 Скачать лист» (`.gl-save` в `.gl-head`)
качает `.txt`-транскрипт и копирует его в буфер (`saveTranscript`/`transcriptText`).
Имя кнопки НЕ содержит «Сохранить»/«Экспорт» — иначе strict-mode у тех тестов словил бы
лишнее совпадение. Фикс: определены токены `--fw-semi/--fw-bold` в `App.svelte` (раньше
`font:`-шорткаты с ними молча сбрасывались). **Движок-тулинг:** новый
`engine-train relay`/`agree` + `.MAT`-импорт (`logasai.rs`) для спарринга с LogasAI —
см. `docs/logasai-benchmark.md §5`. (Сам движок не трогали: pruning замерян lossless.)
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
  `.ranked li.best`, `.ailast .minidie`, `.import`, `.save-go`,
  `.open-res`/`.opening`, `.ptnum` (число пункта). v0.9.11: центральные кнопки
  живут в `.board .board-actions` (Play-snippet, классы `.act/.act-row/.act-puck`);
  `.gamelog` шапка теперь `.gl-head` (`.gl-summary` + `.gl-save`). `.board .rollzone`
  единственная и имеет `aria-label="Бросить кости"`. v0.9.12: шкала шансов и обзор
  ходов сворачиваемы; пипсы — в `.pips` сразу под доской (live-смоук проверяет
  `.pips`, не `.meta`). v0.9.13: «⚡ авто-бросок» (тогл в `.pips`). v0.9.14: сдача —
  ОДНА кнопка `.resign-btn` («🏳 Сдаться») → форма `.resign-confirm` («Да, сдаться» /
  «Отмена»); `<details>`/`.resign summary` и кнопки «Сдаться — оин/с марсом» УБРАНЫ
  (исход определяется позицией). Тексты кнопок: «Начать партию/матч», «Разыграть
  первый ход», «Бросить кости», «Подтвердить ход», «Отменить», «Выкинуть шашку»,
  «Оценка», «Сдаться»/«Да, сдаться», «Удвоить», «Тайк/Пас/Бивер», «Продолжить с этой
  позиции», «Переиграть этот ход», «Понятно, играть», «📄 Скачать лист» (НЕ содержит
  «Сохранить»/«Экспорт» — чтобы не ловить strict-mode тех тестов). Каждая боевая
  кнопка должна быть в ОДНОМ экземпляре (центр ИЛИ панель, не оба). Меняешь разметку
  → синхронно правь spec'и.
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
