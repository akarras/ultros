using System.Reflection;
using System.Security.Cryptography;
using System.Runtime.InteropServices;
using System.Text.Json;

namespace Ultros.Lists;

public static class Native
{
    private const string Library = "ultros_list_doc_ffi";
    private const int MaxResponse = 24 * 1024 * 1024;
    private static string? libraryPath;
    private static nint loaded;
    private static volatile bool ready;
    private static readonly object Gate = new();
    private static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        PropertyNameCaseInsensitive = false,
    };

    static Native() => NativeLibrary.SetDllImportResolver(typeof(Native).Assembly, Resolve);

    public static void Configure(string directory, string? cacheDirectory = null)
    {
        if (!Environment.Is64BitProcess) throw new PlatformNotSupportedException("Ultros requires a 64-bit host.");
        var path = Path.GetFullPath(Path.Combine(directory, "ultros_list_doc_ffi.dll"));
        if (cacheDirectory is not null) path = CacheLibrary(path, cacheDirectory);
        lock (Gate)
        {
            if (libraryPath is not null && !string.Equals(libraryPath, path, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("Restart this host before selecting a different native library.");
            if (!File.Exists(path)) throw new FileNotFoundException("The Ultros Rust DLL is missing.", path);
            libraryPath = path;
            ready = false;
            if (Abi() != 2) throw new InvalidOperationException("Ultros native ABI mismatch.");
            ready = true;
        }
    }

    internal static string CacheLibrary(string source, string directory)
    {
        using var input = File.OpenRead(source);
        if (input.Length is <= 0 or > 64 * 1024 * 1024) throw new IOException("Unexpected Rust library size.");
        var bytes = new byte[checked((int)input.Length)];
        input.ReadExactly(bytes);
        var hash = Convert.ToHexStringLower(SHA256.HashData(bytes));
        Directory.CreateDirectory(directory);
        // One stable cache folder. Immutable filenames separate resident modules
        // without locking the DLL in the Dalamud installation directory.
        var path = Path.GetFullPath(Path.Combine(directory, $"ultros_list_doc_ffi_{hash}.dll"));
        var temporary = path + "." + Guid.NewGuid().ToString("N") + ".pending";
        try
        {
            using (var output = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            { output.Write(bytes); output.Flush(true); }
            try { File.Move(temporary, path, false); }
            catch (IOException) when (File.Exists(path)) { }
            using var existing = File.OpenRead(path);
            if (existing.Length != bytes.Length || !SHA256.HashData(existing).AsSpan().SequenceEqual(SHA256.HashData(bytes)))
                throw new IOException("The cached Rust library failed its integrity check.");
        }
        finally { if (File.Exists(temporary)) File.Delete(temporary); }
        return path;
    }

    private static nint Resolve(string name, Assembly assembly, DllImportSearchPath? searchPath)
    {
        if (name != Library) return nint.Zero;
        lock (Gate)
        {
            if (libraryPath is null) throw new InvalidOperationException("Configure the Ultros Rust bridge first.");
            // Keep the module resident. Finalizers/another load context must never call
            // an unloaded address. The plugin uses an immutable cached copy so
            // the installer can update the original package for the next load.
            return loaded == nint.Zero ? loaded = NativeLibrary.Load(libraryPath) : loaded;
        }
    }

    internal static Reply Call(object request)
    {
        if (!ready) throw new InvalidOperationException("The Rust bridge did not initialize. Check its DLL and ABI, then restart the host.");
        var input = JsonSerializer.SerializeToUtf8Bytes(request, Json);
        var response = Request(input, (nuint)input.Length);
        if (response == 0)
            throw new InvalidOperationException("The native bridge is unavailable or its resource limit was reached. Restart the host after a native fault.");
        try
        {
            var length = ResponseLength(response);
            if (length == 0 || length > MaxResponse)
                throw new InvalidOperationException("Invalid native response length.");
            var output = new byte[checked((int)length)];
            if (ResponseCopy(response, output, (nuint)output.Length) != 0)
                throw new InvalidOperationException("Native response could not be copied.");
            var reply = JsonSerializer.Deserialize<Reply>(output, Json)
                ?? throw new InvalidOperationException("Empty native response.");
            if (!reply.Ok) throw new InvalidOperationException(reply.Error ?? "Native operation failed.");
            return reply;
        }
        finally { ResponseFree(response); }
    }

    [DllImport(Library, EntryPoint = "ultros_list_proto_abi", CallingConvention = CallingConvention.Cdecl)]
    private static extern uint Abi();
    [DllImport(Library, EntryPoint = "ultros_list_proto_request", CallingConvention = CallingConvention.Cdecl)]
    private static extern ulong Request([In] byte[] bytes, nuint length);
    [DllImport(Library, EntryPoint = "ultros_list_proto_response_len", CallingConvention = CallingConvention.Cdecl)]
    private static extern nuint ResponseLength(ulong response);
    [DllImport(Library, EntryPoint = "ultros_list_proto_response_copy", CallingConvention = CallingConvention.Cdecl)]
    private static extern uint ResponseCopy(ulong response, [Out] byte[] output, nuint capacity);
    [DllImport(Library, EntryPoint = "ultros_list_proto_response_free", CallingConvention = CallingConvention.Cdecl)]
    private static extern void ResponseFree(ulong response);

    [DllImport(Library, EntryPoint = "ultros_list_proto_close", CallingConvention = CallingConvention.Cdecl)]
    internal static extern uint CloseDocument(ulong doc);

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
public sealed record CartView(string Name, string PeerId, bool CanUndo, bool CanRedo, CartRow[] Rows)
{
    public string Version { get; init; } = "";
    public bool Connected { get; init; }
    public bool Ready { get; init; }
    public bool AwaitingPersist { get; init; }
    public bool RecoveryReview { get; init; }
    public bool HasServerCopy { get; init; }
    public JsonElement? Scope { get; init; }
}
public sealed record SyncRequest(string Version, string? Update);
public sealed record SyncReply(string Version, string Kind, string Payload);
