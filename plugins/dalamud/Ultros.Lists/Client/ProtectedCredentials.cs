using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Text.Json;

namespace Ultros.Client;

/// <summary>Refresh credentials only. Windows DPAPI; Wine and unverified hosts stay in memory.</summary>
internal sealed class ProtectedCredentials
{
    private readonly string path;
    private readonly string origin;
    private readonly byte[] entropy;
    internal bool CanPersist { get; }
    internal string Mode { get; }
    private sealed record Envelope(int Version, string Origin, string Ciphertext);

    internal ProtectedCredentials(string directory, UltrosEndpoint endpoint, string profile)
    {
        Directory.CreateDirectory(directory);
        path = Path.Combine(directory, "refresh.protected.json");
        origin = endpoint.Origin.AbsoluteUri;
        entropy = SHA256.HashData(Encoding.UTF8.GetBytes(origin + "/" + profile));
        if (!OperatingSystem.IsWindows() || IsWine())
        {
            Mode = "Memory only — reconnect after restarting the game.";
            return;
        }
        try
        {
            ProtectDirectory(directory);
            var probe = RandomNumberGenerator.GetBytes(32);
            var protectedProbe = Transform(probe, false);
            var unprotected = Transform(protectedProbe, true);
            try
            {
                if (!CryptographicOperations.FixedTimeEquals(probe, unprotected))
                    throw new CryptographicException("Credential backend verification failed.");
                CanPersist = true;
                Mode = "Windows user-protected refresh credential.";
            }
            finally { CryptographicOperations.ZeroMemory(probe); CryptographicOperations.ZeroMemory(unprotected); }
        }
        catch
        {
            Mode = "Memory only — protected storage is unavailable.";
        }
    }

    internal string? Load()
    {
        if (!CanPersist || !File.Exists(path)) return null;
        var info = new FileInfo(path);
        if (info.Length > 16 * 1024) throw new IOException("The protected credential file is invalid.");
        var envelope = JsonSerializer.Deserialize<Envelope>(File.ReadAllBytes(path), Protocol.Json)
            ?? throw new IOException("The protected credential file is invalid.");
        if (envelope.Version != 1 || envelope.Origin != origin) throw new CryptographicException("Credential origin mismatch.");
        var plain = Transform(Convert.FromBase64String(envelope.Ciphertext), true);
        try { return Encoding.UTF8.GetString(plain); }
        finally { CryptographicOperations.ZeroMemory(plain); }
    }

    internal void Save(string refresh)
    {
        if (!CanPersist) return;
        var plain = Encoding.UTF8.GetBytes(refresh);
        try
        {
            var cipher = Transform(plain, false);
            AtomicFile.Write(path, JsonSerializer.SerializeToUtf8Bytes(new Envelope(1, origin, Convert.ToBase64String(cipher)), Protocol.Json));
        }
        finally { CryptographicOperations.ZeroMemory(plain); }
    }
    internal void Clear()
    {
        if (File.Exists(path)) File.Delete(path);
    }

    private static bool IsWine() => OperatingSystem.IsWindows()
        && GetProcAddress(GetModuleHandleW("ntdll.dll"), "wine_get_version") != nint.Zero;

    [SupportedOSPlatform("windows")]
    private static void ProtectDirectory(string directory)
    {
        using var identity = WindowsIdentity.GetCurrent();
        var user = identity.User ?? throw new IOException("Current Windows user is unavailable.");
        var acl = new DirectorySecurity();
        acl.SetOwner(user);
        acl.SetAccessRuleProtection(true, false);
        acl.AddAccessRule(new FileSystemAccessRule(user, FileSystemRights.FullControl,
            InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit, PropagationFlags.None, AccessControlType.Allow));
        new DirectoryInfo(directory).SetAccessControl(acl);
    }

    private byte[] Transform(byte[] input, bool decrypt)
    {
        if (!OperatingSystem.IsWindows()) throw new PlatformNotSupportedException();
        var data = new Blob { Length = input.Length, Data = Marshal.AllocHGlobal(input.Length) };
        var extra = new Blob { Length = entropy.Length, Data = Marshal.AllocHGlobal(entropy.Length) };
        var output = new Blob();
        try
        {
            Marshal.Copy(input, 0, data.Data, input.Length);
            Marshal.Copy(entropy, 0, extra.Data, entropy.Length);
            var ok = decrypt
                ? CryptUnprotectData(ref data, nint.Zero, ref extra, nint.Zero, nint.Zero, 1, out output)
                : CryptProtectData(ref data, null, ref extra, nint.Zero, nint.Zero, 1, out output);
            if (!ok || output.Length is < 1 or > 16 * 1024)
                throw new CryptographicException(new Win32Exception(Marshal.GetLastWin32Error()).Message);
            var bytes = new byte[output.Length];
            Marshal.Copy(output.Data, bytes, 0, bytes.Length);
            return bytes;
        }
        finally
        {
            Zero(data.Data, data.Length); Marshal.FreeHGlobal(data.Data);
            Marshal.FreeHGlobal(extra.Data);
            if (output.Data != nint.Zero) { Zero(output.Data, output.Length); _ = LocalFree(output.Data); }
        }
    }
    private static void Zero(nint data, int length)
    {
        for (var i = 0; i < length; i++) Marshal.WriteByte(data, i, 0);
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct Blob { public int Length; public nint Data; }

    [DllImport("crypt32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CryptProtectData(ref Blob input, string? description, ref Blob entropy, nint reserved, nint prompt, int flags, out Blob output);
    [DllImport("crypt32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CryptUnprotectData(ref Blob input, nint description, ref Blob entropy, nint reserved, nint prompt, int flags, out Blob output);
    [DllImport("kernel32.dll")]
    private static extern nint LocalFree(nint memory);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern nint GetModuleHandleW(string module);
    [DllImport("kernel32.dll", CharSet = CharSet.Ansi, ExactSpelling = true)]
    private static extern nint GetProcAddress(nint module, string name);
}
