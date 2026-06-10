#!/usr/bin/env python3
"""Engine v2 trainer: rollout-supervised phase nets (wildbg-style loop).

Reads the NNF2 feature matrices produced by `engine-train encode`, trains one
MLP per phase (contact / race) with CrossEntropy on rollout soft-labels, and
exports the pair in the exact binary format `engine-core/src/net2.rs` loads
(`NNV2` per net, `NV2P` for the pair) — change them together.

Usage:
  python3 train/train_v2.py \
      --contact data/v2-contact.bin --race data/v2-race.bin \
      --out models/nardy-v2.bin [--epochs 300] [--hidden 300,250,200] \
      [--lr 1e-3] [--batch 1024] [--val 0.1] [--seed 7] \
      [--init models/nardy-v2-prev.bin]

Each input file: b"NNF2" u32 version=1, u32 dim, u32 n, then n rows of
(dim f32 features + 4 f32 target probs), little-endian.
"""

import argparse
import struct
import sys
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn

OUTPUTS = 4  # [win_oin, win_mars, lose_oin, lose_mars]


def load_nnf2(path: str):
    raw = Path(path).read_bytes()
    magic, version, dim, n = struct.unpack_from("<4sIII", raw, 0)
    if magic != b"NNF2" or version != 1:
        sys.exit(f"{path}: not an NNF2 v1 file")
    body = np.frombuffer(raw, dtype="<f4", offset=16)
    width = dim + OUTPUTS
    if body.size != n * width:
        sys.exit(f"{path}: size mismatch (n={n}, dim={dim}, floats={body.size})")
    rows = body.reshape(n, width)
    return rows[:, :dim].copy(), rows[:, dim:].copy()


def build_mlp(input_dim: int, hidden: list[int]) -> nn.Sequential:
    layers: list[nn.Module] = []
    prev = input_dim
    for h in hidden:
        layers += [nn.Linear(prev, h), nn.ReLU()]
        prev = h
    layers.append(nn.Linear(prev, OUTPUTS))  # logits; softmax lives in the loss/export contract
    return nn.Sequential(*layers)


def net_to_bytes(model: nn.Sequential) -> bytes:
    """Serialize to the NNV2 layout of engine-core/src/net2.rs."""
    linears = [m for m in model if isinstance(m, nn.Linear)]
    out = bytearray()
    out += b"NNV2"
    out += struct.pack("<II", 1, len(linears))
    for lin in linears:
        out += struct.pack("<II", lin.in_features, lin.out_features)
    for lin in linears:
        w = lin.weight.detach().cpu().numpy().astype("<f4")  # (out, in) row-major
        b = lin.bias.detach().cpu().numpy().astype("<f4")
        out += w.tobytes()
        out += b.tobytes()
    return bytes(out)


def bytes_to_state(blob: bytes, model: nn.Sequential) -> None:
    """Load NNV2 bytes into a matching nn.Sequential (for --init warm starts)."""
    magic, version, n_layers = struct.unpack_from("<4sII", blob, 0)
    if magic != b"NNV2" or version != 1:
        sys.exit("--init: not an NNV2 v1 blob")
    dims = [struct.unpack_from("<II", blob, 12 + 8 * i) for i in range(n_layers)]
    linears = [m for m in model if isinstance(m, nn.Linear)]
    if len(linears) != n_layers or any(
        (lin.in_features, lin.out_features) != d for lin, d in zip(linears, dims)
    ):
        sys.exit("--init: layer shapes do not match --hidden")
    off = 12 + 8 * n_layers
    for lin, (i, o) in zip(linears, dims):
        w = np.frombuffer(blob, dtype="<f4", count=i * o, offset=off).reshape(o, i)
        off += i * o * 4
        b = np.frombuffer(blob, dtype="<f4", count=o, offset=off)
        off += o * 4
        with torch.no_grad():
            lin.weight.copy_(torch.from_numpy(w.copy()))
            lin.bias.copy_(torch.from_numpy(b.copy()))


def train_one(name, feats, targets, hidden, args, device, init_blob=None):
    n, dim = feats.shape
    if n == 0:
        sys.exit(
            f"{name}: 0 rows — the NNF2 file is empty (small datasets may have "
            "no race rows; generate more positions)"
        )
    gen = torch.Generator().manual_seed(args.seed)
    perm = torch.randperm(n, generator=gen)
    # --val 0 disables validation (and best-val snapshotting) entirely.
    n_val = int(n * args.val) if n > 10 else 0
    val_idx, train_idx = perm[:n_val], perm[n_val:]
    if len(train_idx) == 0:
        sys.exit(f"{name}: --val {args.val} leaves an empty training set (n={n})")

    x = torch.from_numpy(feats)
    y = torch.from_numpy(targets)
    model = build_mlp(dim, hidden).to(device)
    if init_blob is not None:
        bytes_to_state(init_blob, model)
    opt = torch.optim.Adam(model.parameters(), lr=args.lr)

    def soft_ce(logits, target):
        return -(target * torch.log_softmax(logits, dim=1)).sum(dim=1).mean()

    best_val, best_state = float("inf"), None
    xt, yt = x[train_idx].to(device), y[train_idx].to(device)
    xv = x[val_idx].to(device) if n_val else None
    yv = y[val_idx].to(device) if n_val else None
    for epoch in range(args.epochs):
        model.train()
        order = torch.randperm(len(train_idx), generator=gen)
        total = 0.0
        for i in range(0, len(order), args.batch):
            idx = order[i : i + args.batch]
            opt.zero_grad()
            loss = soft_ce(model(xt[idx]), yt[idx])
            loss.backward()
            opt.step()
            total += loss.item() * len(idx)
        if n_val:
            model.eval()
            with torch.no_grad():
                vl = soft_ce(model(xv), yv).item()
            if vl < best_val:
                best_val, best_state = vl, {
                    k: v.detach().clone() for k, v in model.state_dict().items()
                }
        if epoch % max(1, args.epochs // 10) == 0 or epoch == args.epochs - 1:
            msg = f"  [{name}] epoch {epoch + 1:>4}/{args.epochs}  train CE {total / len(train_idx):.4f}"
            if n_val:
                msg += f"  val CE {vl:.4f} (best {best_val:.4f})"
            print(msg, flush=True)
    if best_state is not None:
        model.load_state_dict(best_state)  # keep the best-validation snapshot
    model.eval()
    return model.cpu()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--contact", required=True)
    ap.add_argument("--race", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--epochs", type=int, default=300)
    ap.add_argument("--hidden", default="300,250,200")
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--batch", type=int, default=1024)
    ap.add_argument("--val", type=float, default=0.1)
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--init", default="", help="warm-start from a previous NV2P pair")
    args = ap.parse_args()

    torch.manual_seed(args.seed)
    device = (
        "mps" if torch.backends.mps.is_available()
        else "cuda" if torch.cuda.is_available()
        else "cpu"
    )
    hidden = [int(h) for h in args.hidden.split(",") if h]
    print(f"device {device}, hidden {hidden}, epochs {args.epochs}")

    init_contact = init_race = None
    if args.init:
        blob = Path(args.init).read_bytes()
        if blob[:4] != b"NV2P":
            sys.exit("--init: not an NV2P pair file")
        clen = struct.unpack_from("<I", blob, 8)[0]
        init_contact = blob[12 : 12 + clen]
        rlen = struct.unpack_from("<I", blob, 12 + clen)[0]
        init_race = blob[16 + clen : 16 + clen + rlen]

    cx, cy = load_nnf2(args.contact)
    rx, ry = load_nnf2(args.race)
    print(f"contact: {cx.shape[0]} rows × {cx.shape[1]} features")
    print(f"race:    {rx.shape[0]} rows × {rx.shape[1]} features")
    # Fail BEFORE spending minutes training the first net if the second file
    # is empty (small datasets often produce zero race rows).
    for name, arr in (("contact", cx), ("race", rx)):
        if arr.shape[0] == 0:
            sys.exit(f"{name}: 0 rows in the NNF2 file — generate more positions")

    contact = train_one("contact", cx, cy, hidden, args, device, init_contact)
    race = train_one("race", rx, ry, hidden, args, device, init_race)

    cb, rb = net_to_bytes(contact), net_to_bytes(race)
    pair = b"NV2P" + struct.pack("<I", 1)
    pair += struct.pack("<I", len(cb)) + cb
    pair += struct.pack("<I", len(rb)) + rb
    Path(args.out).write_bytes(pair)
    print(f"wrote {args.out} ({len(pair)} bytes: contact {len(cb)}, race {len(rb)})")

    # Interop probe: `engine-train netbench --v2 <out>` prints the same vectors
    # computed by the Rust loader — they must match to ~1e-6.
    with torch.no_grad():
        for name, model, dim in (("contact", contact, cx.shape[1]), ("race", race, rx.shape[1])):
            x = torch.tensor([[((i * 7) % 23) / 23.0 for i in range(dim)]], dtype=torch.float32)
            p = torch.softmax(model(x), dim=1)[0].tolist()
            print(f"probe {name}: {[round(v, 6) for v in p]}")


if __name__ == "__main__":
    main()
