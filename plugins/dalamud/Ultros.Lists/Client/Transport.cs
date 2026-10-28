using System.Net;
using System.Net.Http.Headers;
using System.Net.WebSockets;
using System.Text.Json;
namespace Ultros.Client;

internal sealed class ApiFailure : Exception
{
    internal int Status { get; }
    internal string Code { get; }
    internal int RetryAfter { get; }
    internal ApiFailure(int status, string code, int retry = 0) : base(status == 426
        ? "Update Ultros in the Dalamud plugin installer, then reload the plugin."
        : $"Ultros request failed ({status}, {code}).")
    { Status = status; Code = code; RetryAfter = retry; }
}
internal sealed class Transport : IDisposable
{
    private const int MaxResponse = 12 * 1024 * 1024;
    private const int MaxListSyncResponse = 24 * 1024 * 1024;
    // A list reply can contain both an 8 MiB base64 version and an 8 MiB
    // binary snapshot encoded as base64. Other endpoints retain their limit.
    internal static int ResponseLimit(string path) =>
        path.StartsWith("/api/v1/plugin/lists/", StringComparison.Ordinal)
        && path.EndsWith("/sync", StringComparison.Ordinal) ? MaxListSyncResponse : MaxResponse;
    private readonly HttpClient client;
    private readonly TimeSpan requestTimeout;
    internal UltrosEndpoint Endpoint { get; }
    internal Transport(UltrosEndpoint endpoint, HttpMessageHandler? handler = null, TimeSpan? requestTimeout = null)
    {
        Endpoint = endpoint;
        this.requestTimeout = requestTimeout ?? TimeSpan.FromSeconds(15);
        client = new HttpClient(handler ?? new HttpClientHandler
        {
            UseCookies = false, AllowAutoRedirect = false,
            AutomaticDecompression = DecompressionMethods.GZip | DecompressionMethods.Brotli,
        }) { Timeout = TimeSpan.FromSeconds(15) };
    }
    internal async Task<T> Send<T>(HttpMethod method, string path, object? body, string? access, CancellationToken cancellation)
    {
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(cancellation);
        deadline.CancelAfter(requestTimeout);
        using var request = new HttpRequestMessage(method, Endpoint.At(path));
        ClientBuild.AddHeaders(request);
        request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
        request.Headers.Add("X-Ultros-Capabilities", "in-game-endpoints-v1");
        if (access is not null) request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", access);
        if (body is not null) request.Content = new ByteArrayContent(JsonSerializer.SerializeToUtf8Bytes(body, Protocol.Json));
        if (request.Content is not null) request.Content.Headers.ContentType = new MediaTypeHeaderValue("application/json");
        using var response = await client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, deadline.Token).ConfigureAwait(false);
        var bytes = await ReadBounded(response.Content, ResponseLimit(path), deadline.Token).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
        {
            var code = "request_failed";
            try
            {
                using var json = JsonDocument.Parse(bytes);
                if (json.RootElement.TryGetProperty("error", out var error))
                {
                    var candidate = error.GetString();
                    if (candidate is { Length: > 0 and <= 64 } && candidate.All(c => c is >= 'a' and <= 'z' or '_'))
                        code = candidate;
                }
            }
            catch (JsonException) { }
            var retry = (int)Math.Clamp(response.Headers.RetryAfter?.Delta?.TotalSeconds ?? 0, 0, 600);
            throw new ApiFailure((int)response.StatusCode, code, retry);
        }
        if (typeof(T) == typeof(bool)) return (T)(object)true;
        return JsonSerializer.Deserialize<T>(bytes, Protocol.Json) ?? throw new InvalidOperationException("Ultros returned an empty response.");
    }
    internal async Task ListenNotifications(string access,Action wake,CancellationToken cancellation)
    {
        using var request=new HttpRequestMessage(HttpMethod.Get,Endpoint.At("/api/v1/plugin/notifications/stream"));
        ClientBuild.AddHeaders(request);
        request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("text/event-stream"));
        request.Headers.Authorization=new AuthenticationHeaderValue("Bearer",access);
        using var response=await client.SendAsync(request,HttpCompletionOption.ResponseHeadersRead,cancellation).ConfigureAwait(false);
        if(!response.IsSuccessStatusCode)throw new ApiFailure((int)response.StatusCode,"notification_stream_unavailable");
        if(response.Content.Headers.ContentType?.MediaType!="text/event-stream")throw new IOException("Unexpected notification stream content type.");
        await using var stream=await response.Content.ReadAsStreamAsync(cancellation).ConfigureAwait(false);
        await NotificationFeed.Read(stream,wake,cancellation).ConfigureAwait(false);
    }
    internal Func<string,int,Action,CancellationToken,Task>? ListListener { get; set; }
    internal async Task ListenList(string access,int list,Action wake,CancellationToken cancellation)
    {
        if(ListListener is { } listener){await listener(access,list,wake,cancellation).ConfigureAwait(false);return;}
        var address=new UriBuilder(Endpoint.At($"/api/v1/plugin/lists/{list}/stream"));
        address.Scheme=address.Scheme=="https"?"wss":"ws";
        using var socket=new ClientWebSocket();
        socket.Options.SetRequestHeader("Authorization","Bearer "+access);
        socket.Options.SetRequestHeader("X-Ultros-Client-Version",ClientBuild.Version.ToString(4));
        socket.Options.SetRequestHeader("X-Ultros-Protocol-Version",ClientBuild.ServerProtocol.ToString(System.Globalization.CultureInfo.InvariantCulture));
        socket.Options.KeepAliveInterval=TimeSpan.FromSeconds(20);
        socket.Options.KeepAliveTimeout=TimeSpan.FromSeconds(30);
        using var invoker=new HttpMessageInvoker(new SocketsHttpHandler{AllowAutoRedirect=false,UseCookies=false});
        using(var handshake=CancellationTokenSource.CreateLinkedTokenSource(cancellation)) {
            handshake.CancelAfter(requestTimeout);
            await socket.ConnectAsync(address.Uri,invoker,handshake.Token).ConfigureAwait(false);
        }
        await ListFeed.Read(socket,list,wake,cancellation).ConfigureAwait(false);
    }
    private static async Task<byte[]> ReadBounded(HttpContent content, int limit, CancellationToken cancellation)
    {
        if (content.Headers.ContentLength > limit) throw new IOException("The Ultros response exceeds the client limit.");
        using var input = await content.ReadAsStreamAsync(cancellation).ConfigureAwait(false);
        using var output = new MemoryStream();
        var buffer = new byte[16 * 1024];
        while (true)
        {
            var count = await input.ReadAsync(buffer, cancellation).ConfigureAwait(false);
            if (count == 0) return output.ToArray();
            if (output.Length + count > limit) throw new IOException("The Ultros response exceeds the client limit.");
            output.Write(buffer, 0, count);
        }
    }
    public void Dispose() => client.Dispose();
}
