# Privacy-Preserving Health-Data Ledger (Prototype)

End-to-end prototype (Rust + ZK proofs + React) that lets researchers query **aggregated statistics** over a **synthetic** health dataset **without exposing any individual record**.

The system is built around a minimal ledger that records:
- dataset commitments
- shard proofs (commitment + bucketed aggregates)
- verified aggregation results

The UI and API never return raw records.

## Repo layout
- `backend/` — Rust REST API + SQLite ledger + dataset/proof generation pipeline
- `zk-proofs/` — Groth16 circuit + prover/verifier (arkworks)
- `frontend/` — Researcher dashboard (Vite + React + TS)

## Prereqs
- Rust toolchain (stable)
- Node.js + npm

Windows note: if you see `link.exe not found` while building Rust crates, install **Visual Studio Build Tools** with the **Desktop development with C++** workload.

## Run locally
1) Backend:
```pwsh path=null start=null
cd backend
cargo run
```
The backend listens on `127.0.0.1:8080` by default (override with `BACKEND_ADDR`).

2) Frontend:
```pwsh path=null start=null
cd frontend
npm install
npm run dev
```
Vite proxies `/api/*` to the backend.

3) Use the UI:
- Click **Create dataset + proofs** (defaults to 1,000,000 synthetic records).
- Wait for status `ready`.
- Run the example query: **Average blood glucose by age range**.

## REST API (high level)
- `POST /api/v1/datasets` — start generating a synthetic dataset + ZK proofs
- `GET /api/v1/datasets/:id` — dataset status/progress + dataset commitment
- `GET /api/v1/datasets/:id/shards?include_proof=true` — page through shard commitments, aggregates, and proofs
- `POST /api/v1/queries` — compute an aggregate (count/sum/mean) for a specific age bucket
- `GET /api/v1/zk/vk` — fetch the Groth16 verifying key
- `POST /api/v1/verify/shard` — verify a single shard proof

## ZK design (what is proven)
This prototype uses **per-shard** proofs to keep circuits reasonably sized.

For each shard of `N=1000` records, the Groth16 circuit proves:
1) The prover knows private records `(age, blood_glucose)`.
2) A public commitment `C_shard` equals `Poseidon(absorb(age, glucose)...)`.
3) Public outputs `(sum_glucose_by_bucket[i], count_by_bucket[i])` match aggregates computed from those private records.

A dataset commitment `C_dataset` is computed as `Poseidon(absorb(C_shard_0, C_shard_1, ...))`.

Privacy guarantee: only **bucketed aggregates** and commitments are public; **no individual record is revealed**.

## Limitations / tradeoffs (documented)
- Filters are limited to a fixed set of age buckets (see `zk-proofs/src/constants.rs`).
- Proofs are per-shard; the query result is verified by verifying all shard proofs backing the dataset.
- Groth16 requires a trusted setup; this prototype generates keys locally (not MPC).

These are explicit prototype choices; the code is structured so you can swap in a transparent system or recursive aggregation later.
