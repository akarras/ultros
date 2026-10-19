using System.Text.Json;
using Ultros.Lists;
namespace Ultros.Client;

/// <summary>Transport/persistence only. Rust owns causal state and recovery decisions.</summary>
internal sealed class ConnectedCart : IDisposable
{
    private readonly string path;
    private readonly string origin;
    private readonly string account;
    internal int Id { get; }
    internal bool CanWrite { get; set; }
    internal bool AccessDenied { get; private set; }
    internal Document Document { get; }
    private sealed record Saved(int Format, string Origin, string Account, int List, JsonElement Checkpoint);

    internal ConnectedCart(Connection connection, int id)
    {
        account = connection.Session?.AccountId ?? throw new InvalidOperationException("Connect first.");
        origin = connection.Endpoint.Origin.AbsoluteUri;
        Id = id;
        var directory = Path.Combine(connection.Directory, "accounts", account, "lists");
        Directory.CreateDirectory(directory);
        path = Path.Combine(directory, id.ToString() + ".json");
        JsonElement? checkpoint = null;
        if (File.Exists(path))
        {
            if (new FileInfo(path).Length > 24 * 1024 * 1024) throw new IOException("Saved cart is too large; its file is preserved.");
            var saved = JsonSerializer.Deserialize<Saved>(File.ReadAllBytes(path), Protocol.Json)
                ?? throw new IOException("Saved cart is invalid; its file is preserved.");
            if (saved.Format != 1 || saved.Origin != origin || saved.Account != account || saved.List != id)
                throw new IOException("Saved cart binding does not match this account and server.");
            checkpoint = saved.Checkpoint;
        }
        Document = Document.OpenOnline(checkpoint);
    }

    internal void Save()
    {
        var checkpoint = Document.Checkpoint();
        var bytes = JsonSerializer.SerializeToUtf8Bytes(new Saved(1, origin, account, Id, checkpoint), Protocol.Json);
        if (bytes.Length > 24 * 1024 * 1024) throw new IOException("Saved cart exceeds its storage limit.");
        AtomicFile.Write(path, bytes);
        Document.Persisted(Document.View.Version);
    }
    internal void Edit(object edit) { Document.Edit(edit); Save(); }
    internal void Backup(string suffix)
    {
        var bytes = JsonSerializer.SerializeToUtf8Bytes(new Saved(1, origin, account, Id, Document.Checkpoint()), Protocol.Json);
        AtomicFile.Write(path + "." + suffix, bytes);
    }
    internal void UseServer()
    {
        Backup("recovery-original");
        Document.UseServer(); Save();
    }
    internal async Task Sync(Connection connection, CancellationToken cancellation)
    {
        if (connection.Session?.AccountId != account) throw new InvalidOperationException("Reconnect the cart's original account.");
        if (Document.View.AwaitingPersist) Save();
        if (Document.View.RecoveryReview) return;
        for (var pass = 0; pass < 3; pass++)
        {
            var (request, requiresSave) = Document.PrepareSync();
            if (requiresSave) Save();
            if(request.Update is not null && (!connection.HasScope("lists:write") || !CanWrite))
                request=request with {Update=null};
            try
            {
                var reply = await connection.Send<SyncReply>(HttpMethod.Post, $"/api/v1/plugin/lists/{Id}/sync", request, cancellation).ConfigureAwait(false);
                Backup("before-sync");
                var status = Document.ReceiveSync(reply);
                if (Document.View.AwaitingPersist) Save();
                if (Document.View.RecoveryReview || status == "snapshot_required") return;
                var next = Document.PrepareSync();
                if (next.RequiresSave) Save();
                if (next.Request.Update is null) return;
                if (!connection.HasScope("lists:write") || !CanWrite) return;
            }
            catch (ApiFailure error) when (error.Code is "list_not_found" or "list_forbidden")
            {
                AccessDenied=true;
                Purge(); throw;
            }
            catch (ApiFailure error) when (error.Code is "metadata_forbidden" or "missing_history")
            {
                Document.SyncError(error.Code); Save();
            }
        }
    }
    internal void Purge()
    {
        foreach (var suffix in new[] { "", ".before-sync", ".recovery-original" })
            if (File.Exists(path + suffix)) File.Delete(path + suffix);
    }
    public void Dispose() => Document.Dispose();
}
