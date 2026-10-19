using System.Text.Json;
namespace Ultros.Client;

public static class ClientBuild
{
    public static readonly Version Version = typeof(ClientBuild).Assembly.GetName().Version
        ?? throw new InvalidOperationException("The Ultros package has no version.");
    public const int ServerProtocol = 1;

    internal static void AddHeaders(HttpRequestMessage request)
    {
        request.Headers.Add("X-Ultros-Client-Version", Version.ToString(4));
        request.Headers.Add("X-Ultros-Protocol-Version", ServerProtocol.ToString(System.Globalization.CultureInfo.InvariantCulture));
    }

    internal static void CheckCapabilities(JsonElement capabilities)
    {
        if (!capabilities.TryGetProperty("protocol", out var protocol) || protocol.GetInt32() != ServerProtocol)
            throw new ApiFailure(426, "client_update_required");
        if (capabilities.TryGetProperty("minimum_client_version", out var minimum)
            && System.Version.TryParse(minimum.GetString(), out var required) && Version < required)
            throw new ApiFailure(426, "client_update_required");
    }
}
