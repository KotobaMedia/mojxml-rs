# Running the web app locally

The web app compiles the Rust parser to WebAssembly and then serves the app with
Vite. Because `pnpm dev` runs the WebAssembly build first, both the JavaScript
and Rust/WebAssembly toolchains must be installed.

## Prerequisites

- [Node.js](https://nodejs.org/en/download) 22 or newer. The current Node.js LTS
  release is recommended.
- [pnpm](https://pnpm.io/installation) 11.20.0. This version is pinned in
  `package.json`.
- The current stable [Rust toolchain](https://www.rust-lang.org/tools/install)
  installed with `rustup`. Rust 1.85 or newer is required because the workspace
  uses Rust 2024 edition.
- The Rust `wasm32-unknown-unknown` compilation target.
- [`wasm-pack`](https://drager.github.io/wasm-pack/installer/).
- A modern browser. An internet connection is also needed to install packages
  and to load the app's basemap.

### 1. Install Node.js

Download and install Node.js 22 or a newer LTS release from the
[official Node.js download page](https://nodejs.org/en/download).

Verify the installation in a new terminal:

```sh
node --version
npm --version
```

`node --version` should report `v22` or newer.

### 2. Install pnpm

The recommended setup uses Corepack. Install or update Corepack, enable pnpm,
and let the `packageManager` field in `package.json` select pnpm 11.20.0:

```sh
npm install --global corepack@latest
corepack enable pnpm
```

Alternatively, install the pinned pnpm version directly:

```sh
npm install --global pnpm@11.20.0
```

Verify it:

```sh
pnpm --version
```

### 3. Install Rust and native build tools

On macOS, first install Apple's command-line build tools:

```sh
xcode-select --install
```

On Debian or Ubuntu Linux, install a compiler and linker:

```sh
sudo apt update
sudo apt install build-essential
```

On Windows, use the installer from the
[official Rust installation page](https://www.rust-lang.org/tools/install).
Install the Visual Studio C++ Build Tools if the installer prompts for them.

Then install Rust with `rustup` on macOS, Linux, or WSL:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Follow the installer prompts, restart the terminal, and ensure the stable
toolchain and WebAssembly target are installed:

```sh
rustup toolchain install stable
rustup default stable
rustup target add wasm32-unknown-unknown
```

Verify the tools:

```sh
rustc --version
cargo --version
```

### 4. Install wasm-pack

With Cargo available, this command works on macOS, Linux, and Windows:

```sh
cargo install wasm-pack --locked
```

Precompiled installers are also available from the
[official wasm-pack installer page](https://drager.github.io/wasm-pack/installer/).

Restart the terminal after installation, then verify that `wasm-pack` is on
your `PATH`:

```sh
wasm-pack --version
```

## Install dependencies and start the app

From the repository root:

```sh
cd web
pnpm install --frozen-lockfile
pnpm dev
```

Open the local URL printed by Vite, normally <http://localhost:5173>. Stop the
development server with `Ctrl+C`.

`pnpm dev` rebuilds the WebAssembly package in `crates/wasm/pkg` before starting
Vite, so Rust changes are included each time the command is restarted.

## Production build and preview

```sh
cd web
pnpm install --frozen-lockfile
pnpm build
pnpm preview
```

The production files are written to `web/dist`. Open the URL printed by the
preview server to test them locally.

## Troubleshooting

### `sh: wasm-pack: command not found`

This means `wasm-pack` is not installed or its executable directory is not on
your `PATH`. Install it with:

```sh
cargo install wasm-pack --locked
```

Then restart the terminal and run:

```sh
wasm-pack --version
```

On macOS or Linux, a Rust installation made with `rustup` normally puts Cargo
binaries in `~/.cargo/bin`. If the command is still missing, load Rust's shell
environment and try again:

```sh
source "$HOME/.cargo/env"
wasm-pack --version
```

### The WebAssembly target is missing

If the build reports that it cannot find `wasm32-unknown-unknown` or the Rust
standard library for that target, install it and retry:

```sh
rustup target add wasm32-unknown-unknown
pnpm dev
```

### The Node.js version is unsupported

Check the installed version:

```sh
node --version
```

Install Node.js 22 or newer, open a new terminal, rerun
`pnpm install --frozen-lockfile`, and then run `pnpm dev` again.
