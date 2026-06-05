<div align="center">

![uutils logo](https://raw.githubusercontent.com/uutils/coreutils/refs/heads/main/docs/src/logo.svg)

# uutils AWK

[![Discord](https://img.shields.io/badge/discord-join-7289DA.svg?logo=discord&longCache=true&style=flat)](https://discord.gg/wQVJbvJ)
[![License](http://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/uutils/awk/blob/main/LICENSE-MIT)
[![License](https://img.shields.io/badge/license-APACHE%202.0-orange.svg)](https://github.com/uutils/awk/blob/main/LICENSE-APACHE)
[![dependency status](https://deps.rs/repo/github/uutils/awk/status.svg)](https://deps.rs/repo/github/uutils/awk)

</div>

---

uutils AWK is a WIP, cross-platform reimplementation of GNU AWK (a.k.a. `gawk`) in
[Rust](http://www.rust-lang.org).

## Goals

uutils AWK aims to be a drop-in replacement for `gawk`. Differences with GNU
are treated as bugs.

Our key objectives include:
- Matching GNU's output (stdout and error code) exactly
- Better error messages
- Best-in-class memory safety
- Improved performance
- Providing comprehensive internationalization support (UTF-8, etc.)
- Extensions when relevant

uutils AWK aims to work on as many platforms as possible, to be able to use the same
utils on Linux, macOS, *BSD, Windows, WASI and other platforms. This ensures, for example,
that scripts can be easily transferred between platforms.

## Requirements

- Rust (`cargo`, `rustc`)

### Rust Version

uutils AWK follows Rust's release channels and is tested against stable, beta and
nightly. The minimum supported Rust version at the moment is the previous stable
version, that is, 1.95.0 at the time of writing.

## State of the Repo

Check out https://github.com/uutils/awk/issues/16.

## Contributing

To contribute to uutils AWK, please see [CONTRIBUTING](https://github.com/uutils/coreutils/blob/main/CONTRIBUTING.md).

## License

uutils AWK is licensed under either the MIT License or the Apache v2.0 License - see the `LICENSE-MIT`, `LICENSE-APACHE` files for details.

GNU AWK is licensed under the GPL 3.0 or later.
