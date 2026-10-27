using System.Text.Json;
namespace Ultros.Client;

/// <summary>Conservative visit/freshness/debounce gate, independent of game memory.</summary>
public sealed class CaptureGate
{
    private string? context;
    private bool fresh;
    private string? candidate;
    private DateTimeOffset since;
    private string? submitted;
    public string Status { get; private set; } = "Waiting for a fresh selling list.";
    public void Reset() {context=null;fresh=false;candidate=null;submitted=null;}
    public PrivateListing[]? Observe(string visit, bool marketUpdate, bool loaded, int expectedCount, PrivateListing[] rows, DateTimeOffset now)
    {
        if(context!=visit){context=visit;fresh=marketUpdate;candidate=null;submitted=null;}
        // A fresh game response is a new observation even when the player
        // confirms the same price. It must supersede public delist/relist data
        // and refresh the website's complete private snapshot.
        if(marketUpdate){fresh=true;candidate=null;submitted=null;}
        if(!fresh){candidate=null;Status="Waiting for the selling window's market update. Close and reopen Markets.";return null;}
        if(!loaded){candidate=null;Status="Waiting for the full retainer market inventory.";return null;}
        if(rows.Length!=expectedCount){candidate=null;Status=$"Waiting for all {expectedCount} listings (read {rows.Length}).";return null;}
        if(rows.Length>20||rows.Select(r=>r.Slot).Distinct().Count()!=rows.Length
            ||rows.Any(r=>r.Slot>=20||r.ItemId<=0||r.Quantity is <1 or >999||r.UnitPrice is <1 or >999999999))
        {candidate=null;Status="Waiting for valid listing slots, quantities and prices.";return null;}
        var identity=JsonSerializer.Serialize(rows,Protocol.Json);
        if(candidate!=identity){candidate=identity;since=now;Status="Waiting for 500 ms of stable listings.";return null;}
        if(now-since<TimeSpan.FromMilliseconds(500))return null;
        if(submitted==identity){Status="Listings unchanged since the last confirmed upload.";return null;}
        Status="Selling list ready to upload.";
        return rows;
    }
    public void Acknowledged(PrivateListing[] rows)=>submitted=JsonSerializer.Serialize(rows,Protocol.Json);
}

/// A market response can precede the selling addon's first readable frame.
/// Keep it briefly, bound to the character, retainer and summoned object.
public sealed class CaptureFreshness
{
    private string? context;
    private DateTimeOffset received;
    public void Signal(string visit, DateTimeOffset now) { context=visit;received=now; }
    public bool Take(string visit, DateTimeOffset now)
    {
        if(context!=visit||now<received||now-received>TimeSpan.FromSeconds(10)){context=null;return false;}
        context=null;return true;
    }
    public void Reset()=>context=null;
}
