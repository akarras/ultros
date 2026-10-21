using System.Net;
using System.Security.Cryptography;
using System.Text;
namespace Ultros.Client;

public sealed class UltrosEndpoint
{
    public Uri Origin { get; }
    public bool IsLocalhost { get; }
    public string Namespace { get; }
    public static bool LocalhostAvailable
    {
        get
        {
#if ULTROS_LOCALHOST_DEBUG
            return true;
#else
            return false;
#endif
        }
    }
    public UltrosEndpoint(bool localhost, string localOrigin = "http://localhost:8080")
    {
        if (localhost && !LocalhostAvailable) throw new InvalidOperationException("This package does not enable localhost development.");
        var origin = new Uri(localhost ? localOrigin : "https://ultros.app", UriKind.Absolute);
        var loopback = origin.Host.Equals("localhost", StringComparison.OrdinalIgnoreCase)
            || IPAddress.TryParse(origin.Host.Trim('[', ']'), out var ip) && IPAddress.IsLoopback(ip);
        if (origin.UserInfo.Length != 0 || origin.Query.Length != 0 || origin.Fragment.Length != 0
            || origin.AbsolutePath != "/" || origin.Scheme is not ("http" or "https")
            || localhost && !loopback || !localhost && origin.Scheme != "https")
            throw new ArgumentException("Use a plain Ultros origin. Development HTTP is limited to literal loopback addresses.");
        Origin = origin;
        IsLocalhost = localhost;
        Namespace = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(origin.AbsoluteUri)))[..32];
    }
    public Uri At(string relative)
    {
        if (!relative.StartsWith('/') || relative.StartsWith("//") || relative.Contains('\\'))
            throw new ArgumentException("Expected an Ultros-relative path.");
        var result = new Uri(Origin, relative);
        if (!SameOrigin(result)) throw new ArgumentException("The request left its configured Ultros origin.");
        return result;
    }
    public bool SameOrigin(Uri value) => value.Scheme == Origin.Scheme
        && value.IdnHost.Equals(Origin.IdnHost, StringComparison.OrdinalIgnoreCase)
        && value.Port == Origin.Port && value.UserInfo.Length == 0;
}
