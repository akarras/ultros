namespace Ultros.Client;
internal static class AtomicFile
{
    internal static void Write(string path, byte[] bytes)
    {
        var temporary = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try
        {
            using (var file = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            {
                file.Write(bytes);
                file.Flush(true);
            }
            File.Move(temporary, path, true);
        }
        finally
        {
            try { if (File.Exists(temporary)) File.Delete(temporary); }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
        }
    }
    internal static string Profile(string value)
    {
        if (value.Length is < 1 or > 32 || value.Any(c => !char.IsAsciiLetterOrDigit(c) && c is not '-' and not '_')
            || new[] { "CON", "PRN", "AUX", "NUL" }.Concat(Enumerable.Range(1,9).SelectMany(i=>new[]{"COM"+i,"LPT"+i})).Contains(value, StringComparer.OrdinalIgnoreCase))
            throw new ArgumentException("Use a profile of 1–32 letters, numbers, underscores, or hyphens.");
        return value;
    }
}
