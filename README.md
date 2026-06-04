# opornik

Браузерная платформа для **длинных нард** (по правилам, близким к ФСНР): игра против
сильного ИИ и людей, анализ позиций с лучшими ходами и вероятностью выигрыша,
обучение и отслеживание прогресса.

Ниша подтверждена: сильный движок длинных нард существует (**LogasAI**), но он
заперт в устаревшем Windows-приложении без веба, онлайна, рейтингов, пазлов и
дашбордов. `opornik` строит современный веб-first аналог.

## Статус

**Фаза 0 — движок-фундамент.** Реализовано ядро правил на Rust (`crates/engine-core`):
модель доски, полная генерация легальных ходов по правилам длинных нард, подсчёт
очков (оин/марс), базовый ИИ, self-play. Поверх — варианты (Traditional /
Nardegammon / Classic / **хачапури** с настраиваемым правилом головы), **куб
удвоений** (удвоение, бивер), **матчи** 7/9/11/13/15/17 с **Кроуфордом** и money
game с **Jacoby**, и каркас анализа (вероятности, ранжирование ходов, решения по
кубу: пас / тайк / тугуд). Правила прошли состязательный аудит.

**Фаза 1 — сила игры (в работе).** Готов движок силы на чистом Rust: кодировка
позиции, нейросеть-оценщик (MLP, forward + сериализация весов под WASM), **self-play
TD-обучение** (крейт `engine-train`, без Python) и **n-ply поиск** (expectiminimax
по 21 броску). Та же сеть работает и нативно, и в браузере. Дальше — масштабирование
обучения, точные таблицы эндшпиля и роллауты. Тесты проходят (ядро + обучение);
ядро собирается под `wasm32`. Подробности — в [docs/roadmap.md](docs/roadmap.md).

## Документация

- [docs/rules.md](docs/rules.md) — спецификация правил длинных нард (ФСНР).
- [docs/architecture.md](docs/architecture.md) — архитектура, слои анализа, стек.
- [docs/roadmap.md](docs/roadmap.md) — план по фазам.

## Сборка и проверка

```bash
cargo test                                   # все тесты
cargo run -p engine-core --example selfplay  # партия эвристического ИИ против себя
cargo check -p engine-core --target wasm32-unknown-unknown  # WASM-готовность

# Обучение нейросети self-play и проверка силы
cargo run --release -p engine-train -- train --games 40000 --out models/nardy-net.bin
cargo run --release -p engine-train -- bench --in models/nardy-net.bin --vs heuristic --plies 2
cargo run --release -p engine-train -- bench --in models/nardy-net.bin --vs random
cargo run --release -p engine-train -- er --in models/nardy-net.bin   # ER против роллаутов
```

## Браузерная версия (Фаза 2)

```bash
cargo install wasm-pack                                   # один раз
wasm-pack build crates/engine-wasm --target web --dev --out-dir ../../web/src/engine/pkg
cd web && npm install && npm run dev                      # открыть http://localhost:5173
```

Сейчас работает играбельный срез: партия против движка (вы — белые), доска,
полоса шансов, выбор силы ИИ (1–3 ply). Движок крутится в Web Worker (WASM),
веса нейросети вшиты в `.wasm`. Рантайм-проверка обёртки: `node scripts/wasm-smoke.cjs`
(после `wasm-pack build … --target nodejs --out-dir pkg-node`).

## Стек

Rust (движок, native + WASM) · Svelte 5 (фронтенд) · Go (бэкенд) · Postgres + Redis ·
PyTorch (офлайн self-play). Обоснование — в [docs/architecture.md](docs/architecture.md).

## Лицензия

MIT, см. [LICENSE](LICENSE).
