using System.Text.Json;
namespace Ultros.Client;

/// <summary>Owned by one serialized worker and one exclusively locked profile.</summary>
internal sealed class Connection : IDisposable
{
    private readonly FileStream profileLock;
    private readonly ProtectedCredentials credentials;
    private readonly string metadataPath;
    private readonly Transport transport;
    private string? refresh;
    private string? access;
    private DateTimeOffset accessExpires;
    private PairingChallenge? pairing;
    private DateTimeOffset nextPoll;
    private DateTimeOffset pairingExpires;
    private string? replacementAccount;
    internal string Directory { get; }
    internal UltrosEndpoint Endpoint => transport.Endpoint;
    internal DeviceSession? Session { get; private set; }
    internal bool Connected => access is not null;
    internal string Status { get; private set; } = "Disconnected";
    internal string StorageMode => credentials.Mode;
    internal string? HumanCode => pairing?.UserCode;
    internal string VerificationUrl => Endpoint.At("/plugin/verify").AbsoluteUri;
    internal string ApprovalUrl => pairing?.VerificationUriComplete ?? VerificationUrl;
    internal bool HasScope(string scope) => Session?.Scopes.Contains(scope) == true;
    private sealed record Metadata(string Origin, DeviceSession Session);

    internal Connection(string root, UltrosEndpoint endpoint, string profile)
    {
        profile = AtomicFile.Profile(profile);
        Directory = Path.Combine(Path.GetFullPath(root), endpoint.Namespace, profile);
        credentials = new ProtectedCredentials(Directory, endpoint, profile);
        profileLock = new FileStream(Path.Combine(Directory, "owner.lock"), FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None);
        metadataPath = Path.Combine(Directory, "session.json");
        transport = new Transport(endpoint);
        try
        {
            if (File.Exists(metadataPath) && new FileInfo(metadataPath).Length <= 32 * 1024)
            {
                var saved = JsonSerializer.Deserialize<Metadata>(File.ReadAllBytes(metadataPath), Protocol.Json);
                if (saved?.Origin == endpoint.Origin.AbsoluteUri && ValidAccount(saved.Session.AccountId))
                    Session = saved.Session;
            }
        }
        catch { Status = "Remembered account metadata could not be loaded."; }
    }

    internal async Task Initialize(CancellationToken cancellation)
    {
        try
        {
            refresh = credentials.Load();
            if (refresh is not null) await Renew(cancellation).ConfigureAwait(false);
        }
        catch (ApiFailure e) when(e.Status==426) { access=null;refresh=null;Status=e.Message; }
        catch { access = null; refresh = null; Status = "Reconnect — the previous session could not be restored."; }
    }

    internal async Task BeginPairing(string label, string[] scopes, CancellationToken cancellation)
    {
        using var capabilities = await transport.Send<JsonDocument>(HttpMethod.Get, "/api/v1/plugin/capabilities", null, null, cancellation).ConfigureAwait(false);
        ClientBuild.CheckCapabilities(capabilities.RootElement);
        var replace = Connected;
        replacementAccount = replace ? Session?.AccountId : null;
        pairing = replace
            ? await Send<PairingChallenge>(HttpMethod.Post, "/api/v1/plugin/session/reauthorize", new { label, scopes }, cancellation).ConfigureAwait(false)
            : await transport.Send<PairingChallenge>(HttpMethod.Post, "/api/v1/plugin/pairings", new { label, scopes }, null, cancellation).ConfigureAwait(false);
        if (pairing.VerificationUri != VerificationUrl || pairing.DeviceCode.Length != 43
            || pairing.UserCode.Length != 9 || pairing.ExpiresIn is < 1 or > 300 || pairing.Interval < 5
            || (pairing.VerificationUriComplete is { } complete && !ValidApprovalLink(complete, VerificationUrl, pairing.DeviceCode)))
        { pairing = null; throw new InvalidOperationException("The server returned an invalid pairing challenge."); }
        pairingExpires = DateTimeOffset.UtcNow.AddSeconds(pairing.ExpiresIn);
        nextPoll = DateTimeOffset.UtcNow.AddSeconds(pairing.Interval);
        Status = "Open the approval page to review this connection. Use the short code if you’re signing in on another device.";
    }
    internal static bool ValidApprovalLink(string complete, string verificationUrl, string deviceCode)
    {
        if (!Uri.TryCreate(complete, UriKind.Absolute, out var uri) || uri.GetLeftPart(UriPartial.Path) != verificationUrl
            || uri.Query.Length != 0 || !uri.Fragment.StartsWith("#pair=", StringComparison.Ordinal)) return false;
        var token = uri.Fragment[6..];
        if (token.Length != 43 || token == deviceCode || token.Any(c => !char.IsAsciiLetterOrDigit(c) && c is not '-' and not '_')) return false;
        try
        {
            var decoded = Convert.FromBase64String(token.Replace('-', '+').Replace('_', '/') + "=");
            return decoded.Length == 32 && Convert.ToBase64String(decoded).TrimEnd('=').Replace('+', '-').Replace('/', '_') == token;
        }
        catch (FormatException) { return false; }
    }
    internal void CancelPairing() { pairing = null; replacementAccount = null; Status = Connected ? "Connected" : "Disconnected"; }

    internal async Task<bool> PollPairing(CancellationToken cancellation)
    {
        if (pairing is null || DateTimeOffset.UtcNow < nextPoll) return false;
        if (DateTimeOffset.UtcNow >= pairingExpires) { CancelPairing(); Status = "The game code expired. Start a new connection."; return false; }
        var pending = pairing;
        try
        {
            var pair = await transport.Send<TokenPair>(HttpMethod.Post, "/api/v1/plugin/pairings/poll",
                new { device_code = pending.DeviceCode }, null, cancellation).ConfigureAwait(false);
            await StoreIssued(pair, replacementAccount, cancellation).ConfigureAwait(false);
            pairing = null; replacementAccount = null;
            return true;
        }
        catch (ApiFailure error) when (error.Code is "authorization_pending" or "slow_down" || error.Status == 429)
        {
            nextPoll = DateTimeOffset.UtcNow.AddSeconds(Math.Max(pending.Interval, error.RetryAfter > 0 ? error.RetryAfter : 5));
        }
        catch (HttpRequestException)
        {
            nextPoll = DateTimeOffset.UtcNow.AddSeconds(Math.Max(10, pending.Interval));
            Status = "Waiting for Ultros; the connection code is still pending.";
        }
        catch (OperationCanceledException) when (!cancellation.IsCancellationRequested)
        {
            nextPoll = DateTimeOffset.UtcNow.AddSeconds(10);
            Status = "Pairing response was uncertain. Retrying cannot issue a second token family.";
        }
        catch
        {
            pairing = null; replacementAccount = null;
            Status = "Connection denied, expired, or already redeemed. Start a new connection.";
            throw;
        }
        return false;
    }

    private async Task StoreIssued(TokenPair pair, string? expectedAccount, CancellationToken cancellation)
    {
        if (pair.TokenType != "Bearer" || pair.AccessToken.Length > 128 || pair.RefreshToken.Length > 128
            || !pair.AccessToken.StartsWith("ult_a1.") || !pair.RefreshToken.StartsWith("ult_r1.")
            || pair.ExpiresIn is < 1 or > 900 || !ValidAccount(pair.AccountId)
            || expectedAccount is not null && pair.AccountId != expectedAccount)
        {
            await TryRevoke(pair.RefreshToken).ConfigureAwait(false);
            throw new InvalidOperationException("Ultros returned an invalid or different-account grant.");
        }
        try
        {
            credentials.Save(pair.RefreshToken);
            var actual = await transport.Send<DeviceSession>(HttpMethod.Get, "/api/v1/plugin/session", null, pair.AccessToken, cancellation).ConfigureAwait(false);
            if (actual.AccountId != pair.AccountId || actual.Id != pair.DeviceId)
                throw new InvalidOperationException("The authenticated device identity changed.");
            AtomicFile.Write(metadataPath, JsonSerializer.SerializeToUtf8Bytes(new Metadata(Endpoint.Origin.AbsoluteUri, actual), Protocol.Json));
            refresh = pair.RefreshToken;
            access = pair.AccessToken;
            accessExpires = DateTimeOffset.UtcNow.AddSeconds(pair.ExpiresIn);
            Session = actual;
            Status = "Connected";
        }
        catch
        {
            await TryRevoke(pair.RefreshToken).ConfigureAwait(false);
            access = null; refresh = null;
            try { credentials.Clear(); } catch { }
            // A challenge was already redeemed. Treat any later failure as terminal,
            // including a timeout verifying or saving the resulting session.
            throw new InvalidOperationException("The issued session could not be verified and saved. Reconnect.");
        }
    }

    private async Task Renew(CancellationToken cancellation)
    {
        var old = refresh ?? throw new InvalidOperationException("Connect this game profile first.");
        try
        {
            // Never retry this POST after a timeout or uncertain response.
            var pair = await transport.Send<TokenPair>(HttpMethod.Post, "/api/v1/plugin/session/refresh",
                new { refresh_token = old }, null, cancellation).ConfigureAwait(false);
            await StoreIssued(pair, Session?.AccountId, cancellation).ConfigureAwait(false);
        }
        catch
        {
            access = null; refresh = null;
            await TryRevoke(old).ConfigureAwait(false);
            try { credentials.Clear(); } catch { }
            Status = "Reconnect — refresh failed or its result was uncertain.";
            throw;
        }
    }

    internal async Task<T> Send<T>(HttpMethod method, string path, object? body, CancellationToken cancellation)
    {
        if (access is null || DateTimeOffset.UtcNow >= accessExpires.AddSeconds(-45))
            await Renew(cancellation).ConfigureAwait(false);
        try { return await transport.Send<T>(method, path, body, access, cancellation).ConfigureAwait(false); }
        catch (ApiFailure error) when (error.Status == 401)
        {
            await Renew(cancellation).ConfigureAwait(false);
            return await transport.Send<T>(method, path, body, access, cancellation).ConfigureAwait(false);
        }
    }
    internal Task<T> Public<T>(string path, CancellationToken cancellation) => transport.Send<T>(HttpMethod.Get, path, null, null, cancellation);
    private bool? notificationStreamSupported;
    private DateTimeOffset capabilitiesCheckAt;
    internal async Task<NotificationFeed?> SubscribeNotifications(Action wake,CancellationToken cancellation)
    {
        if(notificationStreamSupported is null||DateTimeOffset.UtcNow>=capabilitiesCheckAt)
        {
            using var capabilities=await Public<JsonDocument>("/api/v1/plugin/capabilities",cancellation).ConfigureAwait(false);
            notificationStreamSupported=capabilities.RootElement.TryGetProperty("features",out var features)
                &&features.EnumerateArray().Any(feature=>feature.GetString()=="notification-stream");
            capabilitiesCheckAt=DateTimeOffset.UtcNow.AddSeconds(notificationStreamSupported==true?300:60);
        }
        if(notificationStreamSupported!=true)return null;
        if(access is null||DateTimeOffset.UtcNow>=accessExpires.AddSeconds(-45))await Renew(cancellation).ConfigureAwait(false);
        return new NotificationFeed(transport,access!,wake,cancellation);
    }
    internal async Task Disconnect()
    {
        pairing = null;
        if (refresh is not null) await TryRevoke(refresh).ConfigureAwait(false);
        access = null; refresh = null; Session = null;
        credentials.Clear();
        if (File.Exists(metadataPath)) File.Delete(metadataPath);
        Status = "Disconnected";
    }
    internal async Task RevokeMemoryOnlyOnExit()
    {
        pairing = null;
        if (!credentials.CanPersist && refresh is not null) await TryRevoke(refresh).ConfigureAwait(false);
    }
    private async Task TryRevoke(string token)
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(3));
        try { _ = await transport.Send<bool>(HttpMethod.Post, "/api/v1/plugin/session/revoke", new { refresh_token = token }, null, timeout.Token).ConfigureAwait(false); }
        catch { }
    }
    private static bool ValidAccount(string value) => ulong.TryParse(value, out var id) && id > 0 && id.ToString() == value;
    public void Dispose() { access = null; refresh = null; pairing = null; transport.Dispose(); profileLock.Dispose(); }
}
