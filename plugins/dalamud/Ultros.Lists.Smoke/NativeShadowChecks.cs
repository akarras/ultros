using Ultros.Lists;
namespace Ultros.Lists.Smoke;

internal static class NativeShadowChecks
{
    internal static void Run(string nativeDirectory, string testDirectory)
    {
        var source=Path.Combine(testDirectory,"source");
        var cache=Path.Combine(testDirectory,"cache");
        var invalid=Path.Combine(testDirectory,"invalid-cache");
        Directory.CreateDirectory(source);
        var original=Path.Combine(source,"ultros_list_doc_ffi.dll");
        File.Copy(Path.Combine(nativeDirectory,"ultros_list_doc_ffi.dll"),original,true);
        Native.Configure(source,cache);
        using var document=Document.Open();
        document.Edit(new{kind="add",item_id=8,quality="any",need=5});
        // Real Windows file-sharing check while P/Invoke is using the native DLL.
        using(var replaceable=new FileStream(original,FileMode.Open,FileAccess.ReadWrite,FileShare.None))
        { if(replaceable.Length==0)throw new Exception("Missing original library."); }
        File.Copy(Path.Combine(nativeDirectory,"ultros_list_doc_ffi.dll"),original,true);
        document.Edit(new{kind="add_acquired",item_id=8,quality="any",delta=2});
        if(document.View.Rows.Single().Acquired!=2)throw new Exception("Replacing the package broke a resident document.");
        var cached=Native.CacheLibrary(original,cache);
        // Prior releases may legitimately remain in this stable cache folder.
        var cachedFiles=Directory.GetFiles(cache,"*.dll").Order().ToArray();
        if(Native.CacheLibrary(original,cache)!=cached||!cachedFiles.SequenceEqual(Directory.GetFiles(cache,"*.dll").Order()))
            throw new Exception("An unchanged library was copied twice.");
        var invalidFixture=Path.Combine(invalid,Path.GetFileName(cached));
        if(File.Exists(invalidFixture))File.Delete(invalidFixture);
        var corrupted=Native.CacheLibrary(original,invalid);
        File.WriteAllBytes(corrupted,[1,2,3]);
        try{_=Native.CacheLibrary(original,invalid);throw new Exception("Corrupt cached library accepted.");}
        catch(IOException){}
        finally{File.Delete(corrupted);}
        Console.WriteLine("PASS: installed Rust DLL remains replaceable while native documents are active; cache reuse and corrupt-cache rejection.");
    }
}
