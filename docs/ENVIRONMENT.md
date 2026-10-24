# Environment notes

## Normal machine

Install Rust with rustup (stable, recent). Then:

```
cargo test
cargo xtask bundle alive_synth --release
```

The bundle appears in `target/bundled/Alive Synth.vst3`. Copy it to the VST3 folder:
Windows `C:\Program Files\Common Files\VST3`, macOS `~/Library/Audio/Plug-Ins/VST3`, Linux `~/.vst3`.

Useful on demand (all marked `ignored`):

```
cargo test --release cpu_budget -- --ignored --nocapture      # CPU at 16 voices
cargo test --release build_time -- --ignored --nocapture      # texture build time and memory
cargo test dump_parameters -- --ignored --nocapture           # regenerate the parameter table
```

Debug builds enable nih-plug's allocation checker: if anything allocates inside `process()`, the
plugin aborts. Use a debug build in the DAW while developing.

## Inside a Claude chat sandbox (what worked)

- There is no rustup (its host is not reachable) and the Ubuntu package is **Rust 1.75, too old** for
  the current nih-plug dependencies.
- conda-forge is reachable. Install a recent Rust with micromamba:

```
cd /tmp
curl -sSL -o mm.tar.bz2 https://conda.anaconda.org/conda-forge/linux-64/micromamba-2.9.0-0.tar.bz2
mkdir -p mm && tar -xjf mm.tar.bz2 -C mm
export MAMBA_ROOT_PREFIX=/tmp/mamba
/tmp/mm/bin/micromamba create -y -q -n rust -c conda-forge rust
export PATH=/tmp/mamba/envs/rust/bin:$PATH
rustc --version        # 1.99 at the time of writing
```

  If that micromamba file name is gone, look up the current one in
  `https://conda.anaconda.org/conda-forge/linux-64/current_repodata.json`.
- GitHub and crates.io are reachable, so nih-plug (a git dependency) and its crates download fine.
- The shell is `sh`, not bash: brace expansion such as `mkdir -p {a,b}` does **not** work. Use
  `bash -c` or write the paths out.
- There is no audio device and no DAW. Verify with tests, offline renders in tests, the benchmarks above,
  and `nm -D "target/bundled/Alive Synth.vst3/Contents/x86_64-linux/Alive Synth.so" | grep GetPluginFactory`.
- Files for the user go in `/mnt/user-data/outputs`. The zip is made with Python `zipfile`,
  skipping the `target` folder.

## Compiler warnings

The build prints warnings that come from nih-plug's VST3 macro (`nih_export_vst3!`). They are not
from this code and can be ignored. Warnings from this code should be fixed.
