using System.Diagnostics;
using System.Reflection;
using System.Runtime.Loader;
using System.Security.Cryptography;
using System.Text.Json;
using WebAssembly.Runtime;

namespace Ultros.Lists;

/// <summary>
/// Pure-managed host for the AOT-compiled WebAssembly list document engine
/// (<c>UltrosListDoc.WasmIL.dll</c>). Loads into a collectible AssemblyLoadContext
/// with zero native DLLs, zero file locks on disk, and automatic trap isolation.
/// </summary>
public static class Native
{
    public const string ModuleFileName = "UltrosListDoc.WasmIL.dll";
    private const string ModuleTypeName = "UltrosListDocModule";
    private const int MaxResponse = 24 * 1024 * 1024;
    private static readonly object Gate = new();
    private static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        PropertyNameCaseInsensitive = false,
    };

    private static string? modulePath;
    private static WasmModuleLoadContext? loadContext;
    private static Type? moduleType;
    private static WasmInstance? instance;
    private static long currentGeneration;
    private static volatile bool ready;

    internal static long CurrentGeneration
    {
        get { lock (Gate) return currentGeneration; }
    }

    public static void Configure(string directory, string? cacheDirectory = null)
    {
        _ = cacheDirectory; // Retained for caller compatibility; managed IL loads from memory without locking disk files.
        var path = Path.GetFullPath(Path.Combine(directory, ModuleFileName));
        lock (Gate)
        {
            if (ready && modulePath is not null && string.Equals(modulePath, path, StringComparison.OrdinalIgnoreCase))
                return;
            if (!File.Exists(path))
                throw new FileNotFoundException("The Ultros managed document module is missing.", path);
            var bytes = File.ReadAllBytes(path);
            if (bytes.Length is <= 0 or > 64 * 1024 * 1024)
                throw new IOException("Unexpected Ultros document module size.");

            UnloadLocked();

            var alc = new WasmModuleLoadContext(typeof(UnmanagedMemory).Assembly);
            try
            {
                using var stream = new MemoryStream(bytes, writable: false);
                var assembly = alc.LoadFromStream(stream);
                var type = assembly.GetType(ModuleTypeName)
                    ?? throw new InvalidOperationException($"Missing {ModuleTypeName} in {ModuleFileName}.");
                var created = new WasmInstance(type);
                try
                {
                    if (created.Abi() != 2)
                        throw new InvalidOperationException("Ultros document ABI mismatch.");
                }
                catch
                {
                    created.Dispose();
                    throw;
                }

                loadContext = alc;
                moduleType = type;
                instance = created;
                modulePath = path;
                currentGeneration++;
                ready = true;
            }
            catch
            {
                alc.Unload();
                throw;
            }
        }
    }

    /// <summary>
    /// Releases the active WebAssembly instance and unloads its collectible AssemblyLoadContext.
    /// Safe to call during plugin teardown; outstanding finalizers become harmless no-ops.
    /// </summary>
    public static WeakReference? Shutdown()
    {
        lock (Gate)
        {
            return UnloadLocked();
        }
    }

    private static WeakReference? UnloadLocked()
    {
        ready = false;
        currentGeneration++;
        var previousInstance = instance;
        instance = null;
        moduleType = null;
        modulePath = null;
        try { previousInstance?.Dispose(); } catch { }
        var alc = loadContext;
        loadContext = null;
        if (alc is null) return null;
        var weak = new WeakReference(alc);
        alc.Unload();
        return weak;
    }

    internal static (Reply Reply, long Generation) Call(object request, long? expectedGeneration = null)
    {
        var input = JsonSerializer.SerializeToUtf8Bytes(request, Json);
        byte[] output;
        long generation;
        lock (Gate)
        {
            if (!ready || instance is null || moduleType is null)
                throw new InvalidOperationException("The document runtime did not initialize. Reload Ultros and try again.");
            if (expectedGeneration is { } expected && expected != currentGeneration)
                throw new InvalidOperationException("The document runtime was reset after a fault. Reload the list.");

            try
            {
                output = instance.Call(input);
                generation = currentGeneration;
            }
            catch (RuntimeException ex)
            {
                ResetAfterTrapLocked();
                throw new InvalidOperationException($"The managed document engine trapped and was reset cleanly ({ex.GetType().Name}). Reload the list.", ex);
            }
            catch (InvalidOperationException ex) when (ex.Message.StartsWith("WASI proc_exit", StringComparison.Ordinal))
            {
                ResetAfterTrapLocked();
                throw new InvalidOperationException($"The managed document engine exited unexpectedly and was reset cleanly ({ex.Message}). Reload the list.", ex);
            }
        }

        var reply = JsonSerializer.Deserialize<Reply>(output, Json)
            ?? throw new InvalidOperationException("Empty document engine response.");
        if (!reply.Ok) throw new InvalidOperationException(reply.Error ?? "Document operation failed.");
        return (reply, generation);
    }

    internal static uint CloseDocument(ulong doc, long generation)
    {
        lock (Gate)
        {
            if (!ready || instance is null || generation != currentGeneration)
                return 0;
            try
            {
                return instance.Close(doc);
            }
            catch (RuntimeException)
            {
                ResetAfterTrapLocked();
                return 1;
            }
        }
    }

    private static void ResetAfterTrapLocked()
    {
        currentGeneration++;
        var trapped = instance;
        instance = null;
        try { trapped?.Dispose(); } catch { }
        if (moduleType is null)
        {
            ready = false;
            return;
        }
        try
        {
            var fresh = new WasmInstance(moduleType);
            if (fresh.Abi() != 2)
            {
                fresh.Dispose();
                ready = false;
                return;
            }
            instance = fresh;
            ready = true;
        }
        catch
        {
            ready = false;
        }
    }

    private sealed class WasmModuleLoadContext(Assembly webAssembly)
        : AssemblyLoadContext("UltrosListDoc.WasmIL", isCollectible: true)
    {
        protected override Assembly? Load(AssemblyName assemblyName) =>
            string.Equals(assemblyName.Name, "WebAssembly", StringComparison.OrdinalIgnoreCase)
                ? webAssembly
                : null;
    }

    private sealed class WasmInstance : IDisposable
    {
        private readonly IDisposable module;
        private readonly UnmanagedMemory memory;
        private readonly Func<int, int> alloc;
        private readonly Action<int, int> dealloc;
        private readonly Func<int> abi;
        private readonly Func<long, int> close;
        private readonly Func<int, int, long> request;
        private readonly Func<long, int> responseLen;
        private readonly Func<long, int> responsePtr;
        private readonly Action<long> responseFree;

        public WasmInstance(Type type)
        {
            UnmanagedMemory? memRef = null;
            var imports = new Dictionary<string, RuntimeImport>(StringComparer.Ordinal)
            {
                ["wasi_snapshot_preview1::random_get"] = new FunctionImport(new Func<int, int, int>((ptr, len) =>
                {
                    RandomNumberGenerator.Fill(MemSpan(memRef!, ptr, len));
                    return 0;
                })),
                ["wasi_snapshot_preview1::clock_time_get"] = new FunctionImport(new Func<int, long, int, int>((clockId, _, outPtr) =>
                {
                    long ns = clockId == 0
                        ? DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() * 1_000_000L
                        : (long)(Stopwatch.GetTimestamp() * (1_000_000_000.0 / Stopwatch.Frequency));
                    BitConverter.TryWriteBytes(MemSpan(memRef!, outPtr, 8), ns);
                    return 0;
                })),
                ["wasi_snapshot_preview1::environ_sizes_get"] = new FunctionImport(new Func<int, int, int>((countPtr, sizePtr) =>
                {
                    BitConverter.TryWriteBytes(MemSpan(memRef!, countPtr, 4), 0);
                    BitConverter.TryWriteBytes(MemSpan(memRef!, sizePtr, 4), 0);
                    return 0;
                })),
                ["wasi_snapshot_preview1::environ_get"] = new FunctionImport(new Func<int, int, int>((_, _) => 0)),
                ["wasi_snapshot_preview1::fd_write"] = new FunctionImport(new Func<int, int, int, int, int>((_, _, _, nwrittenPtr) =>
                {
                    BitConverter.TryWriteBytes(MemSpan(memRef!, nwrittenPtr, 4), 0);
                    return 8; // EBADF
                })),
                ["wasi_snapshot_preview1::poll_oneoff"] = new FunctionImport(new Func<int, int, int, int, int>((_, _, _, _) => 52)), // ENOSYS
                ["wasi_snapshot_preview1::sched_yield"] = new FunctionImport(new Func<int>(() =>
                {
                    Thread.Yield();
                    return 0;
                })),
                ["wasi_snapshot_preview1::proc_exit"] = new FunctionImport(new Action<int>(code =>
                    throw new InvalidOperationException($"WASI proc_exit({code})"))),
            };

            Func<string, string, RuntimeImport> resolver = (mod, field) =>
            {
                var key = $"{mod}::{field}";
                return imports.TryGetValue(key, out var import)
                    ? import
                    : throw new InvalidOperationException($"Disallowed WASM import requested: {key}");
            };

            var obj = Activator.CreateInstance(type, resolver)
                ?? throw new InvalidOperationException($"Could not instantiate {ModuleTypeName}.");
            module = (IDisposable)obj;
            try
            {
                memRef = memory = (UnmanagedMemory)(type.GetProperty("memory")?.GetValue(obj)
                    ?? throw new InvalidOperationException("Missing WASM memory export."));
                type.GetMethod("_initialize")?.Invoke(obj, null);
                alloc = Bind<Func<int, int>>(type, obj, "ultros_list_alloc");
                dealloc = Bind<Action<int, int>>(type, obj, "ultros_list_dealloc");
                abi = Bind<Func<int>>(type, obj, "ultros_list_proto_abi");
                close = Bind<Func<long, int>>(type, obj, "ultros_list_proto_close");
                request = Bind<Func<int, int, long>>(type, obj, "ultros_list_proto_request");
                responseLen = Bind<Func<long, int>>(type, obj, "ultros_list_proto_response_len");
                responsePtr = Bind<Func<long, int>>(type, obj, "ultros_list_proto_response_ptr");
                responseFree = Bind<Action<long>>(type, obj, "ultros_list_proto_response_free");
            }
            catch
            {
                module.Dispose();
                throw;
            }
        }

        private static T Bind<T>(Type type, object instance, string name) where T : Delegate =>
            (type.GetMethod(name) ?? throw new InvalidOperationException($"Missing WASM export '{name}'."))
                .CreateDelegate<T>(instance);

        private static unsafe Span<byte> MemSpan(UnmanagedMemory mem, int offset, int length)
        {
            if (offset < 0 || length < 0 || (ulong)(uint)offset + (ulong)(uint)length > mem.Size)
                throw new MemoryAccessOutOfRangeException((uint)offset, (uint)length);
            return new Span<byte>((byte*)mem.Start + (uint)offset, length);
        }

        public uint Abi() => unchecked((uint)abi());
        public uint Close(ulong doc) => unchecked((uint)close(unchecked((long)doc)));

        public byte[] Call(byte[] input)
        {
            var ptr = input.Length == 0 ? 0 : alloc(input.Length);
            if (ptr == 0 && input.Length > 0)
                throw new InvalidOperationException("The document engine is unavailable or its resource limit was reached.");
            try
            {
                if (input.Length > 0) input.CopyTo(MemSpan(memory, ptr, input.Length));
                var response = request(ptr, input.Length);
                if (response == 0)
                    throw new InvalidOperationException("The document engine is unavailable or its resource limit was reached.");
                try
                {
                    var length = responseLen(response);
                    if (length <= 0 || length > MaxResponse)
                        throw new InvalidOperationException("Invalid document engine response length.");
                    var rptr = responsePtr(response);
                    if (rptr == 0)
                        throw new InvalidOperationException("Document engine response could not be read.");
                    return MemSpan(memory, rptr, length).ToArray();
                }
                finally { responseFree(response); }
            }
            finally { if (ptr != 0) dealloc(ptr, input.Length); }
        }

        public void Dispose() => module.Dispose();
    }

    internal sealed record Reply
    {
        public bool Ok { get; init; }
        public string? Error { get; init; }
        public ulong Doc { get; init; }
        public string? Snapshot { get; init; }
        public CartView? View { get; init; }
        public JsonElement? Checkpoint { get; init; }
        public SyncRequest? Request { get; init; }
        public bool RequiresSave { get; init; }
        public string? Status { get; init; }
    }
}

public sealed record CartRow(int ItemId, string Quality, long Need, long Acquired, long? Target);
public sealed record CartTarget(int RecipeId, long Count, long Completed);
public sealed record CartCraftChoice(int ItemId, string Mode, int? RecipeId, long OnHand, long Crafted);
public sealed record CartView(string Name, string PeerId, bool CanUndo, bool CanRedo, CartRow[] Rows)
{
    public string Version { get; init; } = "";
    public bool Connected { get; init; }
    public bool Ready { get; init; }
    public bool AwaitingPersist { get; init; }
    public bool RecoveryReview { get; init; }
    public bool HasServerCopy { get; init; }
    public JsonElement? Scope { get; init; }
    public CartTarget[] Targets { get; init; } = [];
    public CartCraftChoice[] CraftChoices { get; init; } = [];
    public bool ExcludeCrystals { get; init; }
    public bool IncludeVendors { get; init; }
}
public sealed record SyncRequest(string Version, string? Update);
public sealed record SyncReply(string Version, string Kind, string Payload);
