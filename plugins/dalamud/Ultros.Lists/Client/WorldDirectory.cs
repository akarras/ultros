namespace Ultros.Client;

internal sealed record WorldDirectory(WorldRegion[] Regions)
{
    internal string RegionFor(int current,int home)=>Regions.FirstOrDefault(r=>r.Datacenters.Any(d=>d.Worlds.Any(w=>w.Id==current)))?.Name
        ??Regions.FirstOrDefault(r=>r.Datacenters.Any(d=>d.Worlds.Any(w=>w.Id==home)))?.Name??"";
    internal string? WorldName(int worldId)=>Regions.SelectMany(r=>r.Datacenters).SelectMany(d=>d.Worlds).FirstOrDefault(w=>w.Id==worldId)?.Name;
    internal string? DatacenterName(int worldId)=>Regions.SelectMany(r=>r.Datacenters).FirstOrDefault(d=>d.Worlds.Any(w=>w.Id==worldId))?.Name;
    internal string[] ScopeOptions(int current,int home)
    {
        var list=new List<string>();
        var seen=new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        void Add(string? value){if(!string.IsNullOrWhiteSpace(value)&&seen.Add(value))list.Add(value);}
        var activeWorld=WorldName(current)??WorldName(home);
        var activeDc=DatacenterName(current)??DatacenterName(home);
        var activeRegion=RegionFor(current,home);
        Add(activeRegion);
        Add(activeDc);
        Add(activeWorld);
        foreach(var region in Regions.OrderBy(r=>r.Name,StringComparer.OrdinalIgnoreCase)) Add(region.Name);
        foreach(var dc in Regions.SelectMany(r=>r.Datacenters).OrderBy(d=>d.Name,StringComparer.OrdinalIgnoreCase)) Add(dc.Name);
        foreach(var world in Regions.SelectMany(r=>r.Datacenters).SelectMany(d=>d.Worlds).OrderBy(w=>w.Name,StringComparer.OrdinalIgnoreCase)) Add(world.Name);
        return list.ToArray();
    }
}
internal sealed record WorldRegion(int Id,string Name,WorldDatacenter[] Datacenters);
internal sealed record WorldDatacenter(int Id,string Name,WorldEntry[] Worlds);
internal sealed record WorldEntry(int Id,string Name);

internal static class ListSelection
{
    internal static int? Default(ListAccess[] lists,int? remembered,string account="")=>lists.FirstOrDefault(l=>l.List.Id==remembered)?.List.Id
        ??lists.Where(l=>l.List.Name.Equals("Cart",StringComparison.OrdinalIgnoreCase)).OrderByDescending(l=>l.List.Owner.ToString()==account||l.Permission=="Owner").ThenByDescending(l=>l.CanWrite).ThenBy(l=>l.List.Id).Select(l=>(int?)l.List.Id).FirstOrDefault();
}
