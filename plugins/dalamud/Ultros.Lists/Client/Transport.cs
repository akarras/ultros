using System.Net;
using System.Net.Http.Headers;
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
    private readonly HttpClient client;
    internal UltrosEndpoint Endpoint { get; }
    internal Transport(UltrosEndpoint endpoint, HttpMessageHandler? handler = null)
    {
        Endpoint = endpoint;
        client = new HttpClient(handler ?? new HttpClientHandler
        {
            UseCookies = false, AllowAutoRedirect = false,
            AutomaticDecompression = DecompressionMethods.GZip | DecompressionMethods.Brotli,
        }) { Timeout = TimeSpan.FromSeconds(15) };
    }
    internal async Task<T> Send<T>(HttpMethod method, string path, object? body, string? access, CancellationToken cancellation)
    {
        using var request = new HttpRequestMessage(method, Endpoint.At(path));
        ClientBuild.AddHeaders(request);
        request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
        request.Headers.Add("X-Ultros-Capabilities", "in-game-endpoints-v1");
        if (access is not null) request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", access);
        if (body is not null) request.Content = new ByteArrayContent(JsonSerializer.SerializeToUtf8Bytes(body, Protocol.Json));
        if (request.Content is not null) request.Content.Headers.ContentType = new MediaTypeHeaderValue("application/json");
        using var response = await client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellation).ConfigureAwait(false);
        var bytes = await ReadBounded(response.Content, 12 * 1024 * 1024, cancellation).ConfigureAwait(false);
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
