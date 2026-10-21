namespace Ultros.Lists;

/// <summary>Small local-only persistence shell. Native calls stay on the caller's thread.</summary>
public sealed class LocalCartStore(string root) : IDisposable
{
    private const int MaxSnapshot = 8 * 1024 * 1024;
    private readonly string root = Path.GetFullPath(root);
    private FileStream? profileLock;
    private string? snapshotPath;
    public string Profile { get; private set; } = "";
    public Document? Main { get; private set; }
    public Document? Comparison { get; private set; }
    public bool Dirty { get; private set; }

    public void OpenProfile(string profile)
    {
        if (profile.Length is < 1 or > 32 || profile.Any(c => !char.IsAsciiLetterOrDigit(c) && c is not '-' and not '_'))
            throw new ArgumentException("Profile names use 1–32 letters, numbers, underscores, or hyphens.");
        if (Main is not null && string.Equals(Profile, profile, StringComparison.OrdinalIgnoreCase)) return;
        if (Dirty) throw new InvalidOperationException("Save the current cart before changing profiles.");
        var directory = Path.Combine(root, profile);
        Directory.CreateDirectory(directory);
        var nextLock = new FileStream(Path.Combine(directory, "owner.lock"), FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None);
        Document? next = null;
        try
        {
            var file = Path.Combine(directory, "cart.loro");
            next = Document.Open(File.Exists(file) ? Read(file) : null);
            if (!File.Exists(file)) Write(file, next.Export());
            Comparison?.Dispose(); Comparison = null;
            Main?.Dispose(); profileLock?.Dispose();
            Main = next; next = null;
            profileLock = nextLock;
            snapshotPath = file;
            Profile = profile;
            Dirty = false;
        }
        catch { next?.Dispose(); nextLock.Dispose(); throw; }
    }

    public void Edit(Document document, object edit)
    {
        document.Edit(edit);
        if (ReferenceEquals(document, Main)) { Dirty = true; Save(); }
    }

    public void Save()
    {
        if (Main is null || snapshotPath is null) throw new InvalidOperationException("Open a profile first.");
        Write(snapshotPath, Main.Export());
        Dirty = false;
    }

    public void Reload()
    {
        if (Main is null || snapshotPath is null) throw new InvalidOperationException("Open a profile first.");
        if (Dirty) throw new InvalidOperationException("Save unsaved changes before reopening.");
        var next = Document.Open(Read(snapshotPath));
        Comparison?.Dispose(); Comparison = null;
        Main.Dispose(); Main = next;
    }

    public void Fork()
    {
        var next = Document.Open((Main ?? throw new InvalidOperationException("Open a profile first.")).Export());
        Comparison?.Dispose(); Comparison = next;
    }

    public void LoadComparison(string base64)
    {
        if (base64.Length > MaxSnapshot / 3 * 4 + 8) throw new ArgumentException("Snapshot is too large.");
        var next = Document.Open(Convert.FromBase64String(base64.Trim()));
        Comparison?.Dispose(); Comparison = next;
    }

    public void MergeIntoMain()
    {
        if (Main is null || Comparison is null) throw new InvalidOperationException("Fork peer B first.");
        if (snapshotPath is null) throw new InvalidOperationException("Open a saved profile first.");
        Write(snapshotPath+".before-merge",Main.Export());
        Main.Import(Comparison.Export()); Dirty = true; Save();
    }

    public void MergeIntoComparison()
    {
        if (Main is null || Comparison is null) throw new InvalidOperationException("Fork peer B first.");
        Comparison.Import(Main.Export());
    }

    private static byte[] Read(string path)
    {
        using var file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
        if (file.Length > MaxSnapshot) throw new IOException("Snapshot is larger than 8 MiB; the original file was preserved.");
        var bytes = new byte[checked((int)file.Length)];
        file.ReadExactly(bytes);
        return bytes;
    }

    private static void Write(string path, byte[] bytes)
    {
        if (bytes.Length > MaxSnapshot) throw new IOException("Snapshot exceeds 8 MiB.");
        var temporary = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try
        {
            using (var file = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            {
                file.Write(bytes); file.Flush(flushToDisk: true);
            }
            File.Move(temporary, path, overwrite: true);
        }
        finally { if (File.Exists(temporary)) File.Delete(temporary); }
    }

    public void Dispose()
    {
        try
        {
            if(Dirty)
            {
                try{Save();}
                catch
                {
                    // Preserve a last chance copy without overwriting the previous
                    // durable cart. Cleanup must still release the process lock.
                    try{if(snapshotPath is not null&&Main is not null)Write(snapshotPath+".unsaved-recovery",Main.Export());}catch{ }
                }
            }
        }
        finally
        {
            Comparison?.Dispose(); Main?.Dispose(); profileLock?.Dispose();
            Comparison = null; Main = null; profileLock = null;
        }
    }
}
