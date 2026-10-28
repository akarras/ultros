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
    private TokenPair? pendingVerification;
    private DateTimeOffset nextRestore;
    private PairingChallenge? pairing;
    private DateTimeOffset nextPoll;
    private DateTimeOffset pairingExpires;
    private string? replacementAccount;
    internal string Directory { get; }
    internal UltrosEndpoint Endpoint => transport.Endpoint;
    internal DeviceSession? Session { get; private set; }
    internal bool Connected => access is not null;
    internal bool CanRestore => refresh is not null;
    internal string Status { get; private set; } = "Disconnected";
    internal string StorageMode => credentials.Mode;
    internal string? HumanCode => pairing?.UserCode;
    internal string VerificationUrl => Endpoint.At("/plugin/verify").AbsoluteUri;
    internal string ApprovalUrl => pairing?.VerificationUriComplete ?? VerificationUrl;
    internal bool HasScope(string scope) => Session?.Scopes.Contains(scope) == true;
    private sealed record Metadata(string Origin, DeviceSession Session);

    internal Connection(string root, UltrosEndpoint endpoint, string profile, HttpMessageHandler? handler = null)
    {
        profile = AtomicFile.Profile(profile);
        Directory = Path.Combine(Path.GetFullPath(root), endpoint.Namespace, profile);
        credentials = new ProtectedCredentials(Directory, endpoint, profile);
        profileLock = new FileStream(Path.Combine(Directory, "owner.lock"), FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None);
        metadataPath = Path.Combine(Directory, "session.json");
        transport = new Transport(endpoint, handler);
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
        catch (OperationCanceledException) when (cancellation.IsCancellationRequested) { throw; }
        catch (ApiFailure e) when(e.Status==426) { access=null;Status=e.Message; }
        catch { access = null; Status = CanRestore ? "Waiting for Ultros to restore this connection…" : "Reconnect — the previous session could not be restored."; }
    }

    internal async Task<bool> Restore(CancellationToken cancellation, bool force = false)
    {
        if (Connected || !CanRestore || pairing is not null || !force && DateTimeOffset.UtcNow < nextRestore) return false;
        try { await Renew(cancellation).ConfigureAwait(false); }
        catch (OperationCanceledException) when (cancellation.IsCancellationRequested) { throw; }
        catch (ApiFailure e) when (e.Status == 426) { Status = e.Message; }
        catch { if (CanRestore) Status = "Waiting for Ultros to restore this connection…"; }
        return Connected;
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
            pairing = null;
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
            if (pendingVerification is null) Status = "Waiting for Ultros; the connection code is still pending.";
        }
        catch (OperationCanceledException) when (!cancellation.IsCancellationRequested)
        {
            nextPoll = DateTimeOffset.UtcNow.AddSeconds(10);
            if (pendingVerification is null) Status = "Pairing response was uncertain. Retrying cannot issue a second token family.";
        }
        catch
        {
            pairing = null; replacementAccount = null;
            Status = "Connection denied, expired, or already redeemed. Start a new connection.";
            throw;
        }
        return false;
    }

    internal async Task StoreIssued(TokenPair pair, string? expectedAccount, CancellationToken cancellation)
    {
        if (pair.TokenType != "Bearer" || pair.AccessToken.Length > 128 || pair.RefreshToken.Length > 128
            || !pair.AccessToken.StartsWith("ult_a1.") || !pair.RefreshToken.StartsWith("ult_r1.")
            || pair.ExpiresIn is < 1 or > 900 || !ValidAccount(pair.AccountId)
            || expectedAccount is not null && pair.AccountId != expectedAccount)
        {
            await Forget(pair.RefreshToken).ConfigureAwait(false);
            throw new InvalidOperationException("Ultros returned an invalid or different-account grant.");
        }
        try
        {
            credentials.Save(pair.RefreshToken);
            refresh = pair.RefreshToken;
            access = null;
            accessExpires = DateTimeOffset.UtcNow.AddSeconds(pair.ExpiresIn);
            pendingVerification = pair;
        }
        catch
        {
            await Forget(pair.RefreshToken).ConfigureAwait(false);
            throw new InvalidOperationException("The issued session could not be verified and saved. Reconnect.");
        }
        await VerifyIssued(cancellation).ConfigureAwait(false);
    }

    private async Task VerifyIssued(CancellationToken cancellation)
    {
        var pair = pendingVerification ?? throw new InvalidOperationException("No issued session to verify.");
        DeviceSession actual;
        try { actual = await transport.Send<DeviceSession>(HttpMethod.Get, "/api/v1/plugin/session", null, pair.AccessToken, cancellation).ConfigureAwait(false); }
        catch (Exception error) when (error is HttpRequestException or OperationCanceledException || error is ApiFailure { Status: >= 500 or 429 })
        {
            // The rotation result is known and its NEW credential is saved.
            // A read outage must not revoke its family or retry the old token.
            nextRestore = DateTimeOffset.UtcNow.AddSeconds(15);
            Status = "Waiting for Ultros to verify the saved connection…";
            throw;
        }
        catch { await Forget(pair.RefreshToken).ConfigureAwait(false); throw; }
        try
        {
            if (actual.AccountId != pair.AccountId || actual.Id != pair.DeviceId)
                throw new InvalidOperationException("The authenticated device identity changed.");
            AtomicFile.Write(metadataPath, JsonSerializer.SerializeToUtf8Bytes(new Metadata(Endpoint.Origin.AbsoluteUri, actual), Protocol.Json));
            Session = actual; access = pair.AccessToken; pendingVerification = null; Status = "Connected";
        }
        catch { await Forget(pair.RefreshToken).ConfigureAwait(false); throw; }
    }

    private async Task Forget(string token)
    {
        access = null; refresh = null; pendingVerification = null;
        await TryRevoke(token).ConfigureAwait(false);
        try { credentials.Clear(); } catch { }
        Status = "Reconnect — refresh failed or its result was uncertain.";
    }

    internal async Task Renew(CancellationToken cancellation)
    {
        nextRestore = DateTimeOffset.UtcNow.AddSeconds(15);
        if (pendingVerification is not null && DateTimeOffset.UtcNow < accessExpires.AddSeconds(-45))
        {
            await VerifyIssued(cancellation).ConfigureAwait(false); return;
        }
        pendingVerification = null;
        access = null;
        var old = refresh ?? throw new InvalidOperationException("Connect this game profile first.");
        // No rotation has been sent yet: an offline server or maintenance
        // response here leaves the original credential safe to restore later.
        using (var capabilities = await transport.Send<JsonDocument>(HttpMethod.Get, "/api/v1/plugin/capabilities", null, null, cancellation).ConfigureAwait(false))
            ClientBuild.CheckCapabilities(capabilities.RootElement);
        cancellation.ThrowIfCancellationRequested();
        TokenPair pair;
        try
        {
            // Never retry this POST after a timeout or uncertain response.
            pair = await transport.Send<TokenPair>(HttpMethod.Post, "/api/v1/plugin/session/refresh",
                new { refresh_token = old }, null, cancellation).ConfigureAwait(false);
        }
        catch (ApiFailure error) when(error.Status==426)
        {
            // Compatibility rejected the POST before it consumed the old token.
            // A successful preflight does not prevent a minimum-version change.
            access=null;Status=error.Message;throw;
        }
        catch
        {
            await Forget(old).ConfigureAwait(false);
            throw;
        }
        // Once a usable pair arrives, only its new generation may be retained
        // or revoked. Verification failures must never revoke the old token.
        await StoreIssued(pair, Session?.AccountId, cancellation).ConfigureAwait(false);
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
    private bool? listStreamSupported;
    private DateTimeOffset listCapabilitiesCheckAt;
    private bool? retainerPreferencesSupported;
    private DateTimeOffset retainerCapabilitiesCheckAt;
    internal async Task<bool> RetainerPreferences(CancellationToken cancellation)
    {
        if(retainerPreferencesSupported is null||DateTimeOffset.UtcNow>=retainerCapabilitiesCheckAt) {
            using var capabilities=await Public<JsonDocument>("/api/v1/plugin/capabilities",cancellation).ConfigureAwait(false);
            retainerPreferencesSupported=capabilities.RootElement.TryGetProperty("features",out var features)
                &&features.EnumerateArray().Any(feature=>feature.GetString()=="retainer-preferences");
            retainerCapabilitiesCheckAt=DateTimeOffset.UtcNow.AddSeconds(retainerPreferencesSupported==true?300:60);
        }
        return retainerPreferencesSupported==true;
    }
    internal async Task<ListFeed?> SubscribeList(int list,Action wake,CancellationToken cancellation)
    {
        if(listStreamSupported is null||DateTimeOffset.UtcNow>=listCapabilitiesCheckAt)
        {
            using var capabilities=await Public<JsonDocument>("/api/v1/plugin/capabilities",cancellation).ConfigureAwait(false);
            listStreamSupported=capabilities.RootElement.TryGetProperty("features",out var features)
                &&features.EnumerateArray().Any(feature=>feature.GetString()=="list-stream");
            listCapabilitiesCheckAt=DateTimeOffset.UtcNow.AddSeconds(listStreamSupported==true?300:60);
        }
        if(listStreamSupported!=true)return null;
        if(access is null||DateTimeOffset.UtcNow>=accessExpires.AddSeconds(-45))await Renew(cancellation).ConfigureAwait(false);
        return new ListFeed(transport,access!,list,wake,cancellation);
    }
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
        access = null; refresh = null; pendingVerification = null; Session = null;
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
    public void Dispose() { access = null; refresh = null; pendingVerification = null; pairing = null; transport.Dispose(); profileLock.Dispose(); }
}
