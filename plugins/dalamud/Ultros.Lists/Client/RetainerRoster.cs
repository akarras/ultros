namespace Ultros.Client;

public static class RetainerRoster
{
    /// <summary>Validate the game's display map before using it as an ordering proof.</summary>
    public static int[]? DisplaySlots(ulong[] ids,byte[] order)
    {
        if(ids.Length!=10||order.Length!=10)return null;
        var occupied=Enumerable.Range(0,10).Where(i=>ids[i]!=0).ToArray();
        var sorted=order.Take(occupied.Length).Select(i=>(int)i).ToArray();
        return sorted.Distinct().Count()==occupied.Length&&sorted.All(i=>i<10&&ids[i]!=0)?sorted:null;
    }
    public static int Missing(UnverifiedCharacter[] roster,ObservedCharacter current)
    {
        var saved=ClientWorker.LatestObservation(roster,current);
        return current.Retainers.Count(r=>saved is null||!saved.Retainers.Any(s=>s.Name==r.Name&&(s.Registered||s.GroupingConflict)));
    }
    public static string[]? GameOrder(UnverifiedCharacter saved,ObservedCharacter current)
    {
        if(current.Retainers.Length==0||current.Retainers.Any(r=>r.GameOrder is null or <0 or >9)
            ||current.Retainers.Select(r=>r.GameOrder).Distinct().Count()!=current.Retainers.Length)return null;
        var result=new List<string>();
        foreach(var row in current.Retainers.OrderBy(r=>r.GameOrder))
        {
            var matches=saved.Retainers.Where(r=>r.Registered&&!r.GroupingConflict&&r.Name==row.Name).ToArray();
            if(matches.Length!=1)return null;
            result.Add(matches[0].Id);
        }
        return result.ToArray();
    }
    public static bool Changed(UnverifiedCharacter[] roster,ObservedCharacter current)
    {
        if(ClientWorker.NeedsRegistration(roster,current))return true;
        var saved=ClientWorker.LatestObservation(roster,current);
        return current.Retainers.Any(r=>saved?.Retainers.FirstOrDefault(s=>s.Name==r.Name) is not { } prior
            ||r.GameOrder is { } order&&order!=prior.GameOrder||r.Available is { } available&&available!=prior.Available);
    }
}
