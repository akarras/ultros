using System.Runtime.CompilerServices;
using Ultros.Lists;
using WebAssembly;
using WebAssembly.Instructions;
using WebAssembly.Runtime;

namespace Ultros.Lists.Smoke;

internal static class NativeShadowChecks
{
    internal static void Run(string packageDirectory, string testDirectory)
    {
        var source = Path.Combine(testDirectory, "source");
        Directory.CreateDirectory(source);
        var modulePath = Path.Combine(source, Native.ModuleFileName);
        File.Copy(Path.Combine(packageDirectory, Native.ModuleFileName), modulePath, true);

        // 1. Hot-replacement: managed IL loads from memory so the DLL on disk is never locked.
        Native.Configure(source);
        using var document = Document.Open();
        document.Edit(new { kind = "add", item_id = 8, quality = "any", need = 5 });
        using (var replaceable = new FileStream(modulePath, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
        {
            if (replaceable.Length == 0) throw new Exception("Missing managed document module.");
        }
        File.Copy(Path.Combine(packageDirectory, Native.ModuleFileName), modulePath, true);
        document.Edit(new { kind = "add_acquired", item_id = 8, quality = "any", delta = 2 });
        if (document.View.Rows.Single().Acquired != 2)
            throw new Exception("Replacing the package on disk broke a resident document.");

        // 2. Generation fencing: an old handle must fail cleanly after a runtime reset/reload.
        var staleHandle = Document.Open();
        staleHandle.Edit(new { kind = "add", item_id = 8, quality = "nq", need = 3 });
        document.Dispose();

        // 3. Collectible AssemblyLoadContext unload: releasing the runtime frees the loaded module cleanly.
        var weakAlc = UnloadRuntime();
        if (weakAlc is null) throw new Exception("Expected a collectible AssemblyLoadContext reference.");
        for (var i = 0; i < 12 && weakAlc.IsAlive; i++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
            GC.Collect();
        }
        if (weakAlc.IsAlive)
            throw new Exception("Managed WasmIL AssemblyLoadContext did not unload cleanly.");

        // Re-configure after unload and verify stale handle from previous generation is rejected cleanly.
        Native.Configure(source);
        try
        {
            staleHandle.Refresh();
            throw new Exception("Stale document handle from a previous runtime generation was accepted.");
        }
        catch (InvalidOperationException) { }
        staleHandle.Dispose(); // Disposing a stale handle must be a harmless no-op.

        using var fresh = Document.Open();
        fresh.Edit(new { kind = "add", item_id = 5056, quality = "hq", need = 1 });
        if (fresh.View.Rows.Single().ItemId != 5056)
            throw new Exception("Re-configured WasmIL runtime failed to edit a new document.");
        fresh.Dispose();
        Native.Shutdown();

        // 4. Managed trap containment: a WASM unreachable instruction becomes a catchable .NET exception.
        VerifyWasmTrapContainment();

        Console.WriteLine("PASS: managed WasmIL module stays replaceable on disk, unloads cleanly via collectible ALC, fences stale handles, and contains WASM traps.");
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static WeakReference? UnloadRuntime() => Native.Shutdown();

    private static void VerifyWasmTrapContainment()
    {
        var module = new WebAssembly.Module();
        module.Types.Add(new WebAssemblyType { Returns = [WebAssemblyValueType.Int32] });
        module.Functions.Add(new Function(0));
        module.Exports.Add(new Export("trap", 0));
        module.Codes.Add(new FunctionBody { Code = [new Unreachable(), new End()] });
        using var ms = new MemoryStream();
        module.WriteToBinary(ms);
        ms.Position = 0;
        var creator = Compile.FromBinary<TrapModule>(ms);
        using var inst = creator(new ImportDictionary());
        var trapped = false;
        try { _ = inst.Exports.trap(); }
        catch (UnreachableException) { trapped = true; }
        if (!trapped) throw new Exception("Expected catchable UnreachableException from WASM trap.");
    }
}

public abstract class TrapModule
{
    public abstract int trap();
}
