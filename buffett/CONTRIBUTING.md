MVP Guidelines
===

You can modify whatever you want..

Rust coding conventions
---

* All Rust code is formatted using the latest version of `rustfmt`. Once installed, it will be
  updated automatically when you update the compiler with `rustup`.

* All Rust code is linted with Clippy. If you'd prefer to ignore its advice, do so explicitly:

  ```rust
  #[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
  ```

  Note: Clippy defaults can be overridden in the top-level file `.clippy.toml`.

* For variable names, when in doubt, spell it out. The mapping from type names to variable names
  is to lowercase the type name, putting an underscore before each capital letter. Variable names
  should *not* be abbreviated unless being used as closure arguments and the brevity improves
  readability. When a function has multiple instances of the same type, qualify each with a
  prefix and underscore (i.e. alice_keypair) or a numeric suffix (i.e. tx0).

* For function and method names, use `<verb>_<subject>`. For unit tests, that verb should
  always be `test` and for benchmarks the verb should always be `bench`. Avoid namespacing
  function names with some arbitrary word. Avoid abreviating words in function names.

* As they say, "When in Rome, do as the Romans do." A good patch should acknowledge the coding
  conventions of the code that surrounds it, even in the case where that code has not yet been
  updated to meet the conventions described here.


Terminology
---

Inventing new terms is allowed, but should only be done when the term is widely used and
understood. Avoid introducing new 3-letter terms, which can be confused with 3-letter acronyms.

Some terms we currently use regularly in the codebase:

* fullnode: n. A fully participating network node.
* hash: n. A SHA-256 Hash.
* keypair: n. A Ed25519 key-pair, containing a public and private key.
* pubkey: n. The public key of a Ed25519 key-pair.
* sigverify: v. To verify a Ed25519 digital signature.

