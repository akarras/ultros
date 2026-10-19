using System.Runtime.InteropServices;
using System.Text.Json;

namespace Ultros.Lists;

public sealed class Document : IDisposable
{
    private readonly DocumentHandle handle;
    public CartView View { get; private set; }

    private Document(Native.Reply reply)
    {
        handle = new DocumentHandle(reply.Doc);
        View = reply.View ?? throw new InvalidOperationException("Missing native view.");
    }

    public static Document Open(byte[]? snapshot = null) => new(Native.Call(new
    {
        op = "open", snapshot = snapshot is null ? null : Convert.ToBase64String(snapshot),
    }));

    public static Document OpenOnline(JsonElement? checkpoint = null) => new(Native.Call(new { op = "open_sync", checkpoint }));

    private Native.Reply WithHandle(Func<ulong, object> request)
    {
        var held = false;
        try
        {
            handle.DangerousAddRef(ref held);
            return Native.Call(request(checked((ulong)handle.DangerousGetHandle().ToInt64())));
        }
        finally { if (held) handle.DangerousRelease(); }
    }

    public void Refresh() => View = WithHandle(doc => new { op = "view", doc }).View!;
    public void Edit(object edit) => View = WithHandle(doc => new { op = "edit", doc, edit }).View!;
    public byte[] Export() => Convert.FromBase64String(WithHandle(doc => new { op = "export", doc }).Snapshot!);
    public void Import(byte[] snapshot) => View = WithHandle(doc => new
    {
        op = "import", doc, snapshot = Convert.ToBase64String(snapshot),
    }).View!;
    public void Dispose() => handle.Dispose();
    public (SyncRequest Request, bool RequiresSave) PrepareSync()
    {
        var reply = WithHandle(doc => new { op = "sync_prepare", doc });
        View = reply.View!;
        return (reply.Request ?? throw new InvalidOperationException("Missing sync request."), reply.RequiresSave);
    }
    public string ReceiveSync(SyncReply payload)
    {
        var reply = WithHandle(doc => new { op = "sync_receive", doc, version = payload.Version, kind = payload.Kind, payload = payload.Payload });
        View = reply.View!; return reply.Status ?? "received";
    }
    public JsonElement Checkpoint() => WithHandle(doc => new { op = "sync_checkpoint", doc }).Checkpoint ?? throw new InvalidOperationException("Missing online checkpoint.");
    public void Persisted(string version) => View = WithHandle(doc => new { op = "sync_persisted", doc, version }).View!;
    public void SyncError(string code) => View = WithHandle(doc => new { op = "sync_error", doc, code }).View!;
    public void UseServer() => View = WithHandle(doc => new { op = "sync_use_server", doc }).View!;
    public void RetryRecovery() => View = WithHandle(doc => new { op = "sync_retry", doc }).View!;

    private sealed class DocumentHandle : SafeHandle
    {
        public DocumentHandle(ulong value) : base(nint.Zero, true)
        {
            if (value == 0) throw new InvalidOperationException("Invalid native document handle.");
            SetHandle((nint)checked((long)value));
        }
        public override bool IsInvalid => handle == nint.Zero;
        protected override bool ReleaseHandle()
        {
            try { return Native.CloseDocument(checked((ulong)handle.ToInt64())) == 0; }
            catch { return false; } // A finalizer must never throw into the host.
        }
    }
}
