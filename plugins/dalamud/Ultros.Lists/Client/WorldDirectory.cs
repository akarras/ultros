namespace Ultros.Client;

internal sealed record WorldDirectory(WorldRegion[] Regions)
{
    internal string RegionFor(int current,int home)=>Regions.FirstOrDefault(r=>r.Datacenters.Any(d=>d.Worlds.Any(w=>w.Id==current)))?.Name
        ??Regions.FirstOrDefault(r=>r.Datacenters.Any(d=>d.Worlds.Any(w=>w.Id==home)))?.Name??"";
}
internal sealed record WorldRegion(int Id,string Name,WorldDatacenter[] Datacenters);
internal sealed record WorldDatacenter(int Id,string Name,WorldEntry[] Worlds);
internal sealed record WorldEntry(int Id,string Name);

internal static class ListSelection
{
    internal static int? Default(ListAccess[] lists,int? remembered,string account="")=>lists.FirstOrDefault(l=>l.List.Id==remembered)?.List.Id
        ??lists.Where(l=>l.List.Name.Equals("Cart",StringComparison.OrdinalIgnoreCase)).OrderByDescending(l=>l.List.Owner.ToString()==account||l.Permission=="Owner").ThenByDescending(l=>l.CanWrite).ThenBy(l=>l.List.Id).Select(l=>(int?)l.List.Id).FirstOrDefault();
}
