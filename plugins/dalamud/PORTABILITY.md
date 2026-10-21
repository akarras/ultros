# Document runtime portability

The release targets the Windows x64 process in which Dalamud runs, including when that process runs through Wine/Proton. It uses the installed Dalamud SDK's .NET/CoreCLR environment, not a separately launched Mono application. Our current validation environment is API 15 with .NET 10. The Windows DLL is the artifact to test on Steam Deck and XIV on Mac; a Linux `.so` or macOS `.dylib` cannot replace it through ordinary Windows P/Invoke.

## Current native implementation

`Ultros.Lists.Document` exposes document edits, views, exports and imports to the managed client. Its implementation calls the shared Rust CRDT through a bounded C ABI. Credentials, HTTP and local checkpoint files remain in C#; Rust receives document operations and document bytes only.

The loader copies the installed library to a hash-checked cache before loading it. Existing documents retain their native module for finalization, while the installer can replace the source library. This was tested on Windows with an active document and exclusive replacement of the source DLL.

Windows interop, concurrency, recovery and update checks have passed. **Steam Deck/Proton and XIV on Mac remain untested.** XIV on Mac documents broad plugin support but also known Wine compatibility exceptions; that is not evidence that this plugin works there. See [XIV on Mac's plugin guide](https://www.xivmac.com/user-guide/plugins).

## Possible WebAssembly implementation

.NET's built-in WebAssembly tooling runs .NET in a browser/WASI host. It does not provide a desktop CoreCLR engine for executing an arbitrary Rust `.wasm` module. See [Microsoft's browser WASM documentation](https://learn.microsoft.com/en-us/aspnet/core/client-side/dotnet-interop/wasm-browser-app?view=aspnetcore-10.0) and [.NET's WASI runtime documentation](https://github.com/dotnet/runtime/blob/main/src/mono/wasi/README.md).

Two embedding options deserve different evaluations:

- [Wasmtime's .NET binding](https://github.com/bytecodealliance/wasmtime-dotnet) embeds a mature runtime, but its Windows package contains a native `wasmtime.dll` and JIT. This retains the Windows native dependency and adds an engine. Improved Wine compatibility cannot be assumed.
- A managed engine such as [WACS](https://github.com/kelnishi/WACS) could remove our native library dependency. Supported instructions/imports, memory bounds, cancellation and actual CRDT performance need evaluation. Its existence alone does not establish compatibility or sufficient speed.

A future experiment should keep the same managed document contract and persisted Loro format. The Rust module needs an explicit linear-memory interface and host imports, rather than assuming the website's browser/JavaScript glue runs in Dalamud. It must receive neither credentials nor arbitrary filesystem/network access. Configure memory and execution budgets in the selected engine; WASM alone does not impose useful application limits.

## Acceptance before changing engines

Compare implementations using the same fixtures: concurrent edits and duplicate frames; reopening saved work; compaction/recovery and undo; malformed and oversized snapshots; allocation/resource limits; and plugin unload/update with live documents. Measure startup, peak memory and merge latency for small lists and near-limit documents on Windows, Steam Deck and XIV on Mac. Run all expensive work off the game framework thread.

The native path stays the release implementation until an alternative passes these checks and proves a practical benefit. A managed WASM experiment can be a separate PR without blocking backend integration or changing the on-disk document format.
