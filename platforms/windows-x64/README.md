# colai toolbar — Windows, x86-64

The staged colai toolbar binary for **Windows, x86-64**.

This is not an npm package and you do not install it directly. colai ships by cloning the
plugin (`claude plugin install colai@colai`), and the POSIX launcher at `bin/colai-toolbar`
is what the plugin's `SessionStart` hook runs. On first use, on a Windows x86-64 machine, that launcher
unpacks `bin/colai-toolbar.gz` from this directory, checks it against the SHA-256 in
`bin/colai-toolbar.sha256`, caches the result under a digest-named directory in
`~/.cache/colai/` (resolved by Git for Windows), and execs it. Every launch re-hashes the
cached copy before running it.

See the [main README](<>) for what the toolbar is and what it sends.
