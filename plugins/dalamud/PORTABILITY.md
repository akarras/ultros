# Document runtime portability

The plugin targets Dalamud's .NET 10 / CoreCLR environment (API 15) and ships **zero native binaries**. The shared Rust Loro CRDT document engine (`ultros-list-doc` + `ultros-list-doc-ffi`) is compiled to `wasm32-wasip1` and ahead-of-time translated to pure managed .NET IL (`UltrosListDoc.WasmIL.dll`) at build time using the zero-dependency Apache-2.0 [`WebAssembly`](https://github.com/RyanLamansky/dotnet-webassembly) library.

## Managed WASM-to-IL implementation

`Ultros.Lists.Document` exposes document edits, views, exports, imports and online sync to the managed client. Credentials, HTTP, DPAPI/memory storage and local checkpoint files remain in C#; the WASM-to-IL module receives document operations and document bytes only.

Key properties enforced at build and load time:

- **Zero native DLLs**: Only pure managed `.NET` assemblies (`Ultros.dll`, `Ultros.Lists.dll`, `UltrosListDoc.WasmIL.dll`, and `WebAssembly.dll`) are packaged in `Ultros.zip`.
- **Capability allowlist**: `Ultros.Lists.WasmCompiler` rejects any `.wasm` binary whose imports extend beyond the 8 minimal `wasi_snapshot_preview1` primitives (`random_get`, `clock_time_get`, `environ_sizes_get`, `environ_get`, `fd_write` stub, `poll_oneoff` stub, `sched_yield`, `proc_exit`). No filesystem, socket, or browser/JS imports exist.
- **Hard memory ceiling**: Linked with `--max-memory=134217728` (128 MiB / 2,048 pages), verified at build time. Typical working linear memory stays under 4 MiB.
- **Clean unloading & hot reload**: `Native.Configure` loads `UltrosListDoc.WasmIL.dll` from memory into a collectible `AssemblyLoadContext` bound to the plugin's `WebAssembly.dll` instance. The DLL on disk is never locked during updates, and `Native.Shutdown()` unloads the context cleanly during plugin disposal.
- **Trap containment & generation fencing**: A Rust trap inside the WASM module throws a catchable managed `WebAssembly.Runtime.RuntimeException` instead of terminating the game process. The host disposes the trapped instance, increments the generation counter so stale `Document` handles fail closed rather than aliasing new handles, and instantiates a clean replacement instance (~6 ms).

## Platform status

Windows unit, concurrency, recovery, compatibility-fixture, collectible-ALC unload, and trap-containment checks have passed. Because the document runtime is pure managed .NET IL, it no longer requires a Windows x64 native PE library on Wine/Proton or macOS. However, **full in-game capture and storage testing on Steam Deck/Proton and XIV on Mac remains untested**, so Windows remains the officially supported platform until verified in game. See [XIV on Mac's plugin guide](https://www.xivmac.com/user-guide/plugins) for general Wine caveats.
