using System.Text.Json;
using Ultros.Lists;

namespace Ultros.Client;

/// <summary>Local history belongs to one account and device. Deletion also suppresses redelivery.</summary>
public sealed class NotificationHistory
{
    private sealed record Deleted(string Id, DateTimeOffset ExpiresAt);
    private sealed record Received(string Id, DateTimeOffset At);
    private sealed record Saved(GameDelivery[] Entries, string[] Shown, string[]? Read = null, Deleted[]? Deleted = null, Received[]? ReceivedAt = null);
    private readonly string path;
    private Saved saved;
    public NotificationHistory(string path)
    {
        this.path=path;
        if(File.Exists(path)&&new FileInfo(path).Length>2*1024*1024)
            throw new IOException("Local notification history exceeds its size limit. The original file has been preserved.");
        saved=File.Exists(path)?JsonSerializer.Deserialize<Saved>(File.ReadAllBytes(path),Protocol.Json)??new([],[]):new([],[]);
        if(saved.Entries is null||saved.Shown is null)throw new JsonException("Incomplete notification history.");
    }
    public GameDelivery[] Entries=>Volatile.Read(ref saved).Entries.ToArray();
    public string[] ReadIds=>Volatile.Read(ref saved).Read??[];
    public IReadOnlyDictionary<string, DateTimeOffset> ReceivedTimes=>
        (Volatile.Read(ref saved).ReceivedAt??[]).DistinctBy(r=>r.Id).ToDictionary(r=>r.Id,r=>r.At);
    public bool IsDeleted(string id)=>Volatile.Read(ref saved).Deleted?.Any(d=>d.Id==id)==true;
    public bool WasShown(string id)=>Volatile.Read(ref saved).Shown.Contains(id);
    public void Receive(IEnumerable<GameDelivery> deliveries,DateTimeOffset now)
    {
        var deleted=(saved.Deleted??[]).Where(d=>d.ExpiresAt>now).ToDictionary(d=>d.Id);
        var received=(saved.ReceivedAt??[]).DistinctBy(r=>r.Id).ToDictionary(r=>r.Id,r=>r.At);
        var incoming=deliveries.Where(d=>d.ExpiresAt>now&&!deleted.ContainsKey(d.Id)).ToArray();
        foreach(var item in incoming)received.TryAdd(item.Id,now);
        var entries=saved.Entries.Concat(incoming).DistinctBy(d=>d.Id).ToArray();
        foreach(var old in entries.SkipLast(100).Where(d=>d.ExpiresAt>now))deleted[old.Id]=new(old.Id,old.ExpiresAt);
        var kept=entries.TakeLast(100).ToArray();
        var nextReceived=kept.Where(d=>received.ContainsKey(d.Id)).Select(d=>new Received(d.Id,received[d.Id])).ToArray();
        Save(new(kept,saved.Shown,saved.Read,deleted.Values.ToArray(),nextReceived));
    }
    public void MarkShown(string id)
    {
        if(saved.Entries.Any(d=>d.Id==id))Save(saved with{Shown=saved.Shown.Append(id).Distinct().ToArray()});
    }
    public void MarkRead(string id)
    {
        if(saved.Entries.Any(d=>d.Id==id))Save(saved with{Read=ReadIds.Append(id).Distinct().ToArray()});
    }
    public void Delete(IEnumerable<string> ids,DateTimeOffset now)
    {
        var selected=ids.ToHashSet();
        var deleted=(saved.Deleted??[]).Where(d=>d.ExpiresAt>now)
            .Concat(saved.Entries.Where(d=>selected.Contains(d.Id)&&d.ExpiresAt>now).Select(d=>new Deleted(d.Id,d.ExpiresAt)))
            .DistinctBy(d=>d.Id).ToArray();
        Save(new(saved.Entries.Where(d=>!selected.Contains(d.Id)).ToArray(),saved.Shown,saved.Read,deleted,saved.ReceivedAt));
    }
    private void Save(Saved next)
    {
        var retained=next.Entries.Select(d=>d.Id).ToHashSet();
        next=next with{
            Shown=next.Shown.Where(retained.Contains).ToArray(),
            Read=(next.Read??[]).Where(retained.Contains).ToArray(),
            ReceivedAt=(next.ReceivedAt??[]).Where(r=>retained.Contains(r.Id)).DistinctBy(r=>r.Id).ToArray()
        };
        var bytes=JsonSerializer.SerializeToUtf8Bytes(next,Protocol.Json);
        if(bytes.Length>2*1024*1024)throw new IOException("Notification history is full. Retry after pending deliveries expire.");
        // Publish only after the checkpoint succeeds, so a failed delete remains retryable.
        AtomicFile.Write(path,bytes);
        Volatile.Write(ref saved,next);
    }
}
