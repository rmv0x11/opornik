# Engine v2 training (wildbg-style rollout-supervised loop)

Стек: позиции и роллаут-метки генерирует Rust (`engine-train`), обучение — PyTorch
(минуты на поколение, MPS), веса экспортируются в бинарный формат, который читает
`engine-core/src/net2.rs` (`NNV2`/`NV2P`). Формат описан в обоих местах — менять
синхронно. Интероп-проверка: `probe contact/race` в выводе тренера должен совпадать
с выводом `engine-train netbench --v2 <pair.bin>` до ~1e-6.

## Окружение (один раз)

```bash
/opt/homebrew/bin/python3.14 -m venv train/.venv   # torch не имеет колёс под 3.15
train/.venv/bin/pip install torch numpy
```

## Одно поколение

```bash
# 1. Датасет: self-play позиции текущего поколения + роллаут-метки (ночной прогон
#    для 10k×432; --exclude держит регрессионный eval-set вне обучения)
./target/release/engine-train dataset --net models/<gen-N>.bin \
    --out data/gen-N.tsv --positions 10000 --trials 432 --seed <S> \
    --exclude data/evalset-v1.tsv

# 2. TSV → бинарные матрицы фич (contact 217 / race 196, роутинг = has_contact,
#    как в боевом диспетчере PhaseNets)
./target/release/engine-train encode --in data/gen-N.tsv --out-prefix data/gen-N

# 3. Обучение двух фазовых сетей + экспорт пары (CE на soft-labels, 300-250-200)
train/.venv/bin/python train/train_v2.py \
    --contact data/gen-N-contact.bin --race data/gen-N-race.bin \
    --out models/nardy-v2-genN.bin [--init models/nardy-v2-genN-1.bin]

# 4. Проверка интеропа + скорость
./target/release/engine-train netbench --v2 models/nardy-v2-genN.bin

# 5. Метрики поколения: ER на фиксированном eval-set + дуэль против чемпиона
#    (er/duel для PhaseNets — этап 2; пока ER меряется только для v1-сетей)
```

Скорость (2026-06-11, M-серия, 1 поток): v1 196-80-3 ≈ 7.3 µs/eval;
v2 217-300-250-200-4 ≈ 88 µs/eval (×12.1). Бюджет плана: ~×10 поглощается
top-4 root pruning + SIMD в forward (этап 4).
