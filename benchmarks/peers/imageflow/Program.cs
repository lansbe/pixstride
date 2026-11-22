using Imageflow.Server;

var builder = WebApplication.CreateBuilder(args);
builder.Logging.SetMinimumLevel(LogLevel.Warning);
var app = builder.Build();
app.UseImageflow(new ImageflowMiddlewareOptions()
    .SetMyOpenSourceProjectUrl("https://github.com/lansbe/pixstride")
    .SetMapWebRoot(false)
    .MapPath("/images/", "/images")
    .SetAllowCaching(false)
    .SetAllowDiskCaching(false)
    .SetDefaultCacheControlString("no-store"));
app.Run();
