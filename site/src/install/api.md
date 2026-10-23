# API reference

**[Rust API reference (rustdoc)](../api/open_stomp/index.html)**: every module, type and function of the
`open-stomp` crate, built from the source with all features on.

To build it locally:

```sh
cargo doc --open
```

The Python package has typed stubs (`.pyi`) for the same API, so editors show the signatures and docstrings.
Browse them in [`bindings/python/python/open_stomp`](https://github.com/JackMechem/open-stomp-lib/tree/main/bindings/python/python/open_stomp),
or in Python:

```python
import open_stomp
help(open_stomp.Device)
```
