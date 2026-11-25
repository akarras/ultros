using System.Reflection;
using WebAssembly;
using WebAssembly.Runtime;

// Build-time compiler: validates the wasm32-wasip1 list document module's
// capability boundary and compiles it into a pure-managed .NET IL assembly.
if (args.Length is < 2 or > 3)
    throw new ArgumentException("Usage: Ultros.Lists.WasmCompiler <input.wasm> <output.dll> [assemblyVersion]");

var inputPath = Path.GetFullPath(args[0]);
var outputPath = Path.GetFullPath(args[1]);
var versionText = args.Length == 3 ? args[2] : "1.0.0.0";
var version = Version.Parse(versionText);

// Strict capability allowlist: no filesystem, no sockets, no JS/browser glue.
var allowedImports = new HashSet<string>(StringComparer.Ordinal)
{
    "wasi_snapshot_preview1::random_get",
    "wasi_snapshot_preview1::clock_time_get",
    "wasi_snapshot_preview1::environ_sizes_get",
    "wasi_snapshot_preview1::environ_get",
    "wasi_snapshot_preview1::fd_write",
    "wasi_snapshot_preview1::poll_oneoff",
    "wasi_snapshot_preview1::sched_yield",
    "wasi_snapshot_preview1::proc_exit",
};

const uint maxAllowedPages = 2048; // 128 MiB linear memory hard cap.

using (var inspect = File.OpenRead(inputPath))
{
    var module = WebAssembly.Module.ReadFromBinary(inspect);
    if (module.Memories.Count != 1)
        throw new InvalidOperationException($"Expected exactly 1 linear memory, found {module.Memories.Count}.");
    var limits = module.Memories[0].ResizableLimits;
    if (limits.Maximum is not { } maxPages || maxPages == 0 || maxPages > maxAllowedPages)
        throw new InvalidOperationException($"WASM linear memory must declare a maximum <= {maxAllowedPages} pages (128 MiB); found {limits.Maximum?.ToString() ?? "unbounded"}.");

    foreach (var import in module.Imports)
    {
        var key = $"{import.Module}::{import.Field}";
        if (!allowedImports.Contains(key))
            throw new InvalidOperationException($"Disallowed WASM import '{key}' ({import.Kind}). Refusing to compile.");
    }
}

Directory.CreateDirectory(Path.GetDirectoryName(outputPath)!);
var tempOutput = outputPath + "." + Guid.NewGuid().ToString("N") + ".pending";
try
{
    using (var input = File.OpenRead(inputPath))
    {
        var assemblyName = new AssemblyName("UltrosListDoc.WasmIL") { Version = version };
        var config = new PersistedCompilerConfiguration(
            typeof(object).Assembly,
            typeof(UnmanagedMemory).Assembly,
            assemblyName,
            "UltrosListDoc.WasmIL.dll")
        {
            TypeName = "UltrosListDocModule",
        };
        var builder = Compile.CreatePersistedAssembly(input, config);
        builder.Save(tempOutput);
    }
    File.Move(tempOutput, outputPath, overwrite: true);
    Console.WriteLine($"Compiled managed WASM->IL assembly: {outputPath} ({new FileInfo(outputPath).Length:N0} bytes, v{version})");
}
finally
{
    if (File.Exists(tempOutput)) File.Delete(tempOutput);
}
