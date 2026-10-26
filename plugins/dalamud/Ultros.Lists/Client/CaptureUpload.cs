namespace Ultros.Client;

public sealed record CaptureUploadResult(bool Saved, bool Retryable = false, int RetryAfterSeconds = 15);

/// <summary>One in-memory receipt, retried without changing its identity or body.</summary>
public sealed class CaptureUpload(string context, ListingCapture capture)
{
    private Task<CaptureUploadResult>? sending;
    private DateTimeOffset retryAt;
    public string Context { get; } = context;
    public ListingCapture Capture { get; } = capture;
    public CaptureUploadResult? Result { get; private set; }
    public string Status { get; private set; } = "Waiting to upload private listings.";

    // Called only by the game framework thread. The worker completes immutable results.
    public bool Advance(DateTimeOffset now, Func<ListingCapture, Task<CaptureUploadResult>?> send)
    {
        if(Result is {Saved:true} or {Retryable:false})return true;
        if(sending is not null) {
            if(!sending.IsCompleted)return false;
            Result=sending.GetAwaiter().GetResult();sending=null;
            if(Result.Saved){Status="Private retainer listing snapshot uploaded.";return true;}
            if(!Result.Retryable){Status="Upload needs a fresh selling list. Close and reopen Markets.";return true;}
            retryAt=now.AddSeconds(Math.Clamp(Result.RetryAfterSeconds,1,600));
            Status="Listing upload interrupted. Retrying this snapshot shortly…";
        }
        if(now-Capture.ObservedAt>TimeSpan.FromMinutes(10)) {
            Result=new(false);Status="The snapshot expired. Close and reopen Markets.";return true;
        }
        if(now<retryAt)return false;
        sending=send(Capture);
        if(sending is null){retryAt=now.AddSeconds(1);Status="Uploader queue is full. Waiting before retry.";}
        else Status="Uploading a complete private listing snapshot…";
        return false;
    }
}
