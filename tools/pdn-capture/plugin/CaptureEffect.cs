using System.Drawing;
using System.Drawing.Imaging;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Security.Cryptography;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.Effects.Gpu;
using PaintDotNet.Imaging;
using PaintDotNet.PropertySystem;
using PaintDotNet.Rendering;

namespace OpenPainter.Capture;

public sealed class CaptureEffect : Effect
{
    private readonly object gate = new();
    private string? completedJob;
    public CaptureEffect() : base("OpenPainter Capture", null, new EffectOptions()) { }

    public override void Render(EffectConfigToken? token, RenderArgs dstArgs, RenderArgs srcArgs,
        Rectangle[] rois, int startIndex, int length)
    {
        dstArgs.Surface.CopySurface(srcArgs.Surface, rois, startIndex, length);
        var path = Environment.GetEnvironmentVariable("OPENPAINTER_CAPTURE_JOB");
        if (string.IsNullOrWhiteSpace(path)) return;
        lock (gate)
        {
            if (completedJob == path) return;
            completedJob = path;
            RunJob(path, srcArgs.Surface);
        }
    }

    private void RunJob(string path, Surface source)
    {
        CaptureJob? job = null;
        string? resultPath = null;
        string? donePath = null;
        try
        {
            job = JsonSerializer.Deserialize<CaptureJob>(File.ReadAllText(path), JsonOptions)
                ?? throw new InvalidDataException("Job JSON is empty.");
            resultPath = job.ResultPath;
            donePath = job.DonePath;
            if (string.IsNullOrWhiteSpace(resultPath) || string.IsNullOrWhiteSpace(donePath))
                throw new InvalidDataException("resultPath and donePath are required.");
            var host = (IEffect)this;
            var service = host.Services.GetService(typeof(IEffectsService2)) as IEffectsService2
                ?? throw new NotSupportedException("IEffectsService2 was not supplied by paint.net.");
            var infos = service.EffectInfos.Where(i => i.IsBuiltIn).ToArray();
            if (job.Mode == "catalog")
                File.WriteAllText(job.OutputPath, JsonSerializer.Serialize(infos.Select(i => TryDescribe(i, host)), JsonOptions));
            else if (job.Mode == "api")
                File.WriteAllText(job.OutputPath, DescribePublicApi(infos, host));
            else if (job.Mode is "capture" or "identity")
            {
                using var current = source.Clone();
                using var bitmap = current.CreateAliasedBitmap();
                bitmap.Save(job.OutputPath, ImageFormat.Png);
            }
            else throw new InvalidDataException($"Unknown mode: {job.Mode}");
            using var sourceBitmap = source.CreateAliasedBitmap();
            File.WriteAllText(resultPath, JsonSerializer.Serialize(new { ok = true, outputPath = job.OutputPath,
                width = source.Width, height = source.Height, layerCount = 1,
                layerCountSource = "one PNG opened in a fresh isolated process",
                surfaceSha256 = SurfaceHash(sourceBitmap) }, JsonOptions));
        }
        catch (Exception error)
        {
            resultPath ??= job?.ResultPath ?? path + ".result.json";
            File.WriteAllText(resultPath, JsonSerializer.Serialize(new { ok = false, error = error.ToString() }, JsonOptions));
        }
        finally
        {
            if (donePath is not null) File.WriteAllText(donePath, "done");
        }
    }

    private static string SurfaceHash(Bitmap bitmap)
    {
        var rect = new Rectangle(0, 0, bitmap.Width, bitmap.Height);
        var data = bitmap.LockBits(rect, ImageLockMode.ReadOnly, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
        try
        {
            using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            var row = new byte[bitmap.Width * 4];
            for (var y = 0; y < bitmap.Height; y++)
            {
                System.Runtime.InteropServices.Marshal.Copy(data.Scan0 + y * data.Stride, row, 0, row.Length);
                hash.AppendData(row);
            }
            return Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant();
        }
        finally { bitmap.UnlockBits(data); }
    }

    private static object Describe(IEffectInfo2 info, IEffect host)
    {
        var effect = info.CreateInstance(host.Services, host.Environment);
        try
        {
            var properties = Properties(effect);
            return new { id = Id(info), info.Name, category = info.Category.ToString(), info.SubmenuName,
                parameters = properties?.Properties.Select(p => new { name = p.Name,
                    type = p.ValueType.FullName, defaultValue = Value(p.DefaultValue),
                    min = Minimum(p), max = Maximum(p),
                    choices = (p as StaticListChoiceProperty)?.ValueChoices.Select(Value).ToArray() }).ToArray(),
                schemaAvailable = properties is not null };
        }
        finally { (effect as IDisposable)?.Dispose(); }
    }

    // The public contract only. Built-in effects arrive as closed proxies, so
    // a token comes from the effect's own config form, and pixels from a
    // renderer it offers. The form is built on an STA thread and never shown.
    private static T OnSta<T>(Func<T> work)
    {
        T? value = default;
        Exception? failure = null;
        var thread = new Thread(() =>
        {
            try { value = work(); }
            catch (Exception error) { failure = error; }
        });
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        thread.Join();
        if (failure is not null) throw new InvalidOperationException("Config form failed.", failure);
        return value!;
    }

    // The token exists only once the form has loaded, so the dialog opens on
    // the hidden desktop for a moment, and its default token is read.
    private static EffectConfigToken? TokenOf(IEffect effect) => OnSta(() =>
    {
        var form = effect.CreateConfigForm();
        form.SetEffect(effect);
        EffectConfigToken? token = null;
        Exception? failure = null;
        using var timer = new System.Windows.Forms.Timer { Interval = 1500 };
        timer.Tick += (_, _) =>
        {
            timer.Stop();
            try { token = form.Token?.Clone() as EffectConfigToken; }
            catch (Exception error) { failure = error; }
            form.Close();
        };
        timer.Start();
        form.ShowDialog(null);
        if (failure is not null) throw failure;
        (form as IDisposable)?.Dispose();
        return token;
    });

    private static string DescribePublicApi(IEffectInfo2[] infos, IEffect host)
    {
        var rendererTypes = AppDomain.CurrentDomain.GetAssemblies()
            .Where(a => a.GetName().Name?.StartsWith("PaintDotNet", StringComparison.Ordinal) == true)
            .SelectMany(a => a.GetExportedTypes())
            .Where(t => t.IsInterface && typeof(IEffectRenderer).IsAssignableFrom(t))
            .ToArray();
        var rows = new List<object>();
        foreach (var info in infos)
        {
            var effect = info.CreateInstance(host.Services, host.Environment);
            try
            {
                string tokenText;
                try
                {
                    var token = TokenOf(effect);
                    tokenText = token is null ? "none" : token.GetType().FullName + " props=" +
                        ((token as PropertyBasedEffectConfigToken)?.Properties.Count.ToString() ?? "n/a");
                }
                catch (Exception error) { tokenText = "error: " + error.GetBaseException().Message; }
                rows.Add(new { info.Name, renderers = rendererTypes.Where(effect.IsRendererAvailable).Select(t => t.Name).ToArray(), token = tokenText });
            }
            finally { (effect as IDisposable)?.Dispose(); }
        }
        var flags = System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Instance;
        var form = typeof(IEffectConfigForm).GetMembers(flags).Select(m => m.ToString()).ToArray();
        var bitmap = typeof(IBitmapEffectRenderer).GetMembers(flags).Select(m => m.ToString()).ToArray();
        return JsonSerializer.Serialize(new { rendererTypes = rendererTypes.Select(t => t.FullName), form, bitmap, rows }, JsonOptions);
    }

    private static object TryDescribe(IEffectInfo2 info, IEffect host)
    {
        try { return Describe(info, host); }
        catch (Exception error)
        {
            return new { id = Id(info), info.Name, category = info.Category.ToString(),
                info.SubmenuName, schemaAvailable = false, error = error.ToString() };
        }
    }

    private static PropertyCollection? Properties(IEffect effect) => effect switch
    {
        PropertyBasedBitmapEffect bitmap => bitmap.CreatePropertyCollection(),
        PropertyBasedEffect classic => classic.CreatePropertyCollection(),
        PropertyBasedGpuImageEffect gpuImage => gpuImage.CreatePropertyCollection(),
        PropertyBasedGpuDrawingEffect gpuDrawing => gpuDrawing.CreatePropertyCollection(),
        PropertyBasedGpuEffect gpu => gpu.CreatePropertyCollection(),
        _ => null
    };
    private static object? Minimum(Property p) => p switch
    {
        ScalarProperty<int> i => i.MinValue,
        ScalarProperty<double> d => d.MinValue,
        ScalarProperty<float> f => f.MinValue,
        _ => null
    };
    private static object? Value(object? value) => value switch
    {
        null => null,
        string or bool or byte or short or int or long or float or double or decimal => value,
        Enum e => e.ToString(),
        _ => value.ToString()
    };
    private static object? Maximum(Property p) => p switch
    {
        ScalarProperty<int> i => i.MaxValue,
        ScalarProperty<double> d => d.MaxValue,
        ScalarProperty<float> f => f.MaxValue,
        _ => null
    };
    private static string Id(IEffectInfo2 info) => string.Join("/",
        Path.GetFileNameWithoutExtension(info.AssemblyLocation), info.Category,
        info.SubmenuName ?? "", info.Name).ToLowerInvariant();

    private static readonly JsonSerializerOptions JsonOptions = CreateJsonOptions();
    private static JsonSerializerOptions CreateJsonOptions()
    {
        var options = new JsonSerializerOptions { PropertyNameCaseInsensitive = true, WriteIndented = true };
        options.Converters.Add(new JsonStringEnumConverter());
        return options;
    }
    private sealed record CaptureJob(string Mode, string InputPath, string OutputPath, string ResultPath,
        string DonePath, List<EffectRequest>? Effects);
    private sealed record EffectRequest(string Id, Dictionary<string, JsonElement> Parameters);
}
