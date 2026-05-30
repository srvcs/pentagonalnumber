# srvcs-pentagonalnumber

Sequences microservice for srvcs.cloud: computes the `n`-th **pentagonal
number** `P(n) = n*(3n-1)/2`.

This service is an **orchestrator**. It owns the control flow but delegates the
headline arithmetic to other srvcs primitives over HTTP:

1. compute `m = 3*n - 1` locally (index arithmetic);
2. ask `srvcs-multiply` for `p = n * m`;
3. ask `srvcs-divide` for `result = p / 2`.

It does **not** call `srvcs-isnumber` directly — input validation propagates
from its dependencies (their `422`s are forwarded).

## API

### `GET /`

Service identity.

```json
{
  "service": "srvcs-pentagonalnumber",
  "concern": "sequences: nth pentagonal number",
  "depends_on": ["srvcs-multiply", "srvcs-divide"]
}
```

### `POST /`

Request:

```json
{ "value": 5 }
```

Response `200`:

```json
{ "value": 5, "result": 35 }
```

`P(5) = 5*(3*5-1)/2 = 5*14/2 = 35`, and `P(1) = 1`.

Other statuses:

- `422` — a dependency rejected the input (forwarded verbatim).
- `500` — a dependency returned a malformed result (missing integer `result`).
- `503` — a dependency is unavailable (degraded).

## Configuration

| Variable             | Default                  | Description                     |
| -------------------- | ------------------------ | ------------------------------- |
| `SRVCS_BIND_ADDR`    | `0.0.0.0:8080`           | Listen address.                 |
| `SRVCS_MULTIPLY_URL` | `http://127.0.0.1:8090`  | Base URL of `srvcs-multiply`.   |
| `SRVCS_DIVIDE_URL`   | `http://127.0.0.1:8091`  | Base URL of `srvcs-divide`.     |

## Local checks

```sh
nix flake check -L
nix develop -c sh -euc 'cargo fmt --check; cargo clippy --all-targets -- -D warnings; cargo test'
nix build .#default -L
```

The Linux container is exposed as `.#container`. On Apple Silicon, use
`linux/arm64` for the practical local check; CI builds the release image on
native `x86_64-linux`.

```sh
docker run --rm --platform linux/arm64 -v "$PWD":/workspace -w /workspace nixos/nix:latest \
  sh -lc 'nix --extra-experimental-features "nix-command flakes" build .#container -L --out-link oci-image && cp -L oci-image image.tar.gz'
```

See [`srvcs/platform`](https://github.com/srvcs/platform) for the shared service
standard and CI workflow.
