# Contributing

Run checks from `engine/`:

```sh
cargo xtask check --tier fast
cargo xtask check --tier milestone
cargo xtask check --tier checkpoint
```

The fast tier runs formatting, Clippy, and workspace tests. The milestone tier also runs release tests and scenario files. The checkpoint tier also runs the app size and launch scripts. A missing Mac app or scenario file prints `SKIP`.

To create a missing golden image, run its test with `OP_BLESS=1`, for example `OP_BLESS=1 cargo test -p op-testkit`. Review the PNG in `tests/goldens/` before keeping it. Image tests allow a difference of 1/255 in each channel and no pixels over that limit. Pixels with zero alpha in both images can have different colors. Record any larger allowance in `tests/tolerances.toml` with a reason.

Ported code must start with a header that names its source file and license. See `THIRD_PARTY.md` for the license record.
