using System.Text.Json;
namespace Ultros.Client;

internal static class Protocol
{
    internal static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        PropertyNameCaseInsensitive = false,
        MaxDepth = 64,
    };
    internal static readonly string[] NotificationScopes = ["notifications:read", "notifications:ack", "notifications:mark-read"];
    internal static readonly string[] DefaultScopes = [.. NotificationScopes, "lists:read"];
}

internal sealed class PairingChallenge
{
    public string DeviceCode { get; init; } = "";
    public string UserCode { get; init; } = "";
    public string VerificationUri { get; init; } = "";
    public string? VerificationUriComplete { get; init; }
    public int ExpiresIn { get; init; }
    public int Interval { get; init; }
    public override string ToString() => "[Pairing credentials redacted]";
}
internal sealed class TokenPair
{
    public string AccessToken { get; init; } = "";
    public string RefreshToken { get; init; } = "";
    public string TokenType { get; init; } = "";
    public int ExpiresIn { get; init; }
    public string DeviceId { get; init; } = "";
    public string AccountId { get; init; } = "";
    public string AccountName { get; init; } = "";
    public string[] Scopes { get; init; } = [];
    public override string ToString() => "[Device credentials redacted]";
}
public sealed record DeviceSession(string Id, string Label, string AccountId, string AccountName, string[] Scopes,
    DateTimeOffset CreatedAt, DateTimeOffset LastSeenAt, DateTimeOffset ExpiresAt, int? EndpointId);
public sealed record ObservedRetainer(string ContentId, string Name, int? City);
public sealed record ObservedCharacter(string ContentId, string Name, int HomeWorld, ObservedRetainer[] Retainers);
public sealed record PrivateRetainer(string Id, string Name, int? City, bool Registered, long Revision, DateTimeOffset ObservedAt, bool GroupingConflict, int? PublicRetainerId = null);
public sealed record UnverifiedCharacter(string Id, string Name, int HomeWorld, bool Registered, bool Verified,
    bool UploadEnabled, long CaptureEpoch, DateTimeOffset ObservedAt, PrivateRetainer[] Retainers);
public sealed record PrivateListing(byte Slot, int ItemId, bool Hq, int Quantity, long UnitPrice);
public sealed record ListingCapture(string CaptureId, long BaseRevision, long CaptureEpoch, DateTimeOffset ObservedAt, bool Complete, PrivateListing[] Listings);
public sealed record CaptureReceipt(long Revision, string Hash);
public sealed record RetainerSnapshot(string RetainerId, long Revision, DateTimeOffset ReceivedAt, DateTimeOffset ObservedAt, PrivateListing[] Listings);
public sealed record GameAction(string Kind, int? ItemId, int? ListId);
public sealed record GameDelivery(string Id, long? EventId, string Kind, string Title, string Body, GameAction Action, DateTimeOffset ExpiresAt);
internal sealed record DeliveryBatch(GameDelivery[] Deliveries, int PollAfterMs);
public sealed record RemoteList(int Id, long Owner, string Name, JsonElement WdrFilter);
public sealed record ListAccess(RemoteList List, string Permission, string? OwnerName)
{
    public bool CanWrite => Permission is "Write" or "Owner";
}
public sealed record MarketRow(string World, string Retainer, bool Hq, int Quantity, long UnitPrice);
public sealed record MarketView(int ItemId, string Scope, MarketRow[] Listings, string Status);
public sealed record SaleRow(bool Hq, int Quantity, long UnitPrice, string SoldAt);
