# @colai/toolbar-linux-x64

The colai toolbar binary for **Linux, x86-64, glibc 2.35 or newer**.

You do not install this package directly. It is an optional dependency of
[`@colai/toolbar`](https://www.npmjs.com/package/@colai/toolbar), and npm installs it only
on a machine whose `os` is `linux` and whose `cpu` is `x64`. Every other
machine gets a different one, or none.

It holds one thing: the toolbar, gzipped. `@colai/toolbar` unpacks it on first use and
checks it against a digest compiled into the wrapper — not against the `.sha256` shipped
here, which anybody able to replace the binary could replace too.

See the [main README](<>) for what the toolbar is and what it
sends.
