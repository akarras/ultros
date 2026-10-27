using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class CaptureUploadChecks
{
    internal static void Run()
    {
        static void Require(bool condition,string message){if(!condition)throw new Exception(message);}
        var now=DateTimeOffset.UtcNow;
        PrivateListing[] rows=[new(0,8,false,5,100)];
        var capture=new ListingCapture("receipt",0,1,now,true,rows);
        var upload=new CaptureUpload("visit",capture);
        var reply=new TaskCompletionSource<CaptureUploadResult>();
        var sent=new List<ListingCapture>();
        Task<CaptureUploadResult>? Send(ListingCapture value){sent.Add(value);return reply.Task;}
        Require(!upload.Advance(now,Send)&&sent.Count==1,"The first upload was not queued.");
        Require(!upload.Advance(now.AddSeconds(1),Send)&&sent.Count==1,"A pending upload was queued twice.");
        reply.SetResult(new(false,true,20));
        Require(!upload.Advance(now.AddSeconds(2),Send),"A transient failure discarded the receipt.");
        Require(!upload.Advance(now.AddSeconds(21),Send)&&sent.Count==1,"Retry ignored server backoff.");
        reply=new();
        Require(!upload.Advance(now.AddSeconds(22),Send)&&sent.Count==2,"A failed upload never retried.");
        Require(ReferenceEquals(sent[0],sent[1]),"Retry changed the receipt identity or body after an uncertain response.");
        reply.SetResult(new(true));
        Require(upload.Advance(now.AddSeconds(23),Send)&&upload.Result?.Saved==true,"The server acknowledgment was lost.");
        Require(upload.Advance(now.AddSeconds(24),_=>throw new Exception("Confirmed receipt sent twice.")),"A completed upload was reopened.");

        var gate=new CaptureGate();
        gate.Observe("visit",true,true,1,rows,now);
        Require(gate.Observe("visit",false,true,1,rows,now.AddSeconds(1)) is not null,"Capture skipped readiness.");
        Require(gate.Observe("visit",false,true,1,rows,now.AddSeconds(2)) is not null,"Queue acceptance incorrectly suppressed unconfirmed rows.");
        gate.Acknowledged(upload.Capture.Listings);
        Require(gate.Observe("visit",false,true,1,rows,now.AddSeconds(24)) is null,"Polling unchanged memory submitted twice.");
        Require(gate.Observe("visit",true,true,1,rows,now.AddSeconds(25)) is null,"A new response skipped stability debounce.");
        Require(gate.Observe("visit",false,true,1,rows,now.AddMilliseconds(25500)) is not null,"Confirming an unchanged price failed to refresh the complete snapshot.");
        gate.Acknowledged(rows);
        Require(gate.Observe("visit",false,true,1,rows,now.AddSeconds(26)) is null,"A fresh observation was submitted more than once.");

        var denied=new CaptureUpload("visit",capture);
        denied.Advance(now,_=>Task.FromResult(new CaptureUploadResult(false)));
        Require(denied.Advance(now.AddSeconds(1),_=>throw new Exception("Denied capture retried.")),"A definitive denial did not require a fresh capture.");
        var expired=new CaptureUpload("visit",capture);
        Require(expired.Advance(now.AddMinutes(11),_=>throw new Exception("Expired capture sent.")),"An expired capture was retained.");
        var full=new CaptureUpload("visit",capture);
        full.Advance(now,_=>null);
        Require(!full.Advance(now.AddMilliseconds(500),_=>throw new Exception("Full queue spun without backoff.")),"A full queue discarded the capture.");
    }
}
