# Managed Function Transpiler

This crate converts a conservative subset of Python or C++ into the Hivemind
managed function syntax.

It is not a general Python or C++ compiler. Unsupported constructs are rejected
explicitly instead of being translated unsafely.

Supported v0 subset:

- one top-level function
- parameters
- local assignments/declarations
- one `if/else` returning expressions
- `return`
- simple expressions preserved as text

Rejected v0 constructs include loops, imports/includes, classes, templates,
exceptions, allocation, macros, and file/network/host access.
