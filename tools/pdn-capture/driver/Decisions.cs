using System.Buffers.Binary;
using System.IO;
using System.Globalization;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace OpenPainter.Capture.Driver;

public sealed record CaptureJob(string Mode, string InputPath, string OutputPath, string ResultPath,
    string DonePath, List<JsonElement>? Effects);

public static class Decisions
{
    private static readonly JsonSerializerOptions JobJson = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = false,
        WriteIndented = true,
    };

    /// <summary>Reads a job file. Every field except effects must be present, spelled exactly.</summary>
    public static CaptureJob ParseJob(string json)
    {
        ValidateJobJson(json);
        using var doc = JsonDocument.Parse(json);
        foreach (var field in new[] { "mode", "inputPath", "outputPath", "resultPath", "donePath" })
            if (!doc.RootElement.TryGetProperty(field, out var value) || value.ValueKind != JsonValueKind.String ||
                string.IsNullOrWhiteSpace(value.GetString()))
                throw new InvalidDataException($"Job field '{field}' is required and must be text.");
        return JsonSerializer.Deserialize<CaptureJob>(json, JobJson)
            ?? throw new InvalidDataException("Job JSON is empty.");
    }

    public static string WriteJob(CaptureJob job) => JsonSerializer.Serialize(job, JobJson);

    private static readonly HashSet<string> NoDialog = new(StringComparer.OrdinalIgnoreCase)
    { "auto-level", "black and white", "invert alpha", "invert colors" };

    public static bool InvocationSucceeded(string name, bool opened, bool closed, string? error)
    {
        if (opened != closed) return false;
        if (!opened && !NoDialog.Contains(name)) return false;
        return error is null || opened && closed && error.Contains("Catastrophic failure", StringComparison.OrdinalIgnoreCase);
    }

    public static void ValidateJobJson(string json)
    {
        using var doc = JsonDocument.Parse(json);
        if (doc.RootElement.ValueKind != JsonValueKind.Object) throw new InvalidDataException("Job must be an object.");
        var allowed = new HashSet<string>(StringComparer.Ordinal) { "mode", "inputPath", "outputPath", "resultPath", "donePath", "effects" };
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var field in doc.RootElement.EnumerateObject())
        {
            if (!allowed.Contains(field.Name)) throw new InvalidDataException($"Unknown job field '{field.Name}'.");
            if (!seen.Add(field.Name)) throw new InvalidDataException($"Duplicate job field '{field.Name}'.");
        }
    }

    public static void ValidateEffectJson(string json, IEnumerable<string> labels)
    {
        using var doc = JsonDocument.Parse(json);
        var effect = doc.RootElement;
        if (effect.ValueKind != JsonValueKind.Object) throw new InvalidDataException("Effect must be an object.");
        foreach (var field in effect.EnumerateObject())
            if (field.Name is not ("id" or "parameters")) throw new InvalidDataException($"Unknown effect field '{field.Name}'.");
        if (!effect.TryGetProperty("id", out var id) || id.ValueKind != JsonValueKind.String)
            throw new InvalidDataException("Effect id is required.");
        if (!effect.TryGetProperty("parameters", out var parameters)) return;
        if (parameters.ValueKind != JsonValueKind.Object) throw new InvalidDataException($"{id.GetString()}: parameters must be an object.");
        var names = labels.ToHashSet(StringComparer.Ordinal);
        foreach (var field in parameters.EnumerateObject())
        {
            if (!names.Contains(field.Name)) throw new InvalidDataException($"{id.GetString()}: unknown number label '{field.Name}'.");
            if (field.Value.ValueKind != JsonValueKind.Number || !field.Value.TryGetDouble(out var n) || !double.IsFinite(n))
                throw new InvalidDataException($"{id.GetString()}: '{field.Name}' must be a finite number.");
        }
    }

    public static void ValidateDistinctPaths(params string[] paths)
    {
        if (paths.Any(p => string.IsNullOrWhiteSpace(p) || !Path.IsPathFullyQualified(p)))
            throw new InvalidDataException("Job paths must be absolute.");
        if (paths.Select(Path.GetFullPath).Distinct(StringComparer.OrdinalIgnoreCase).Count() != paths.Length)
            throw new InvalidDataException("Input, output, result, done, and manifest paths must differ.");
    }

    public static bool CatalogContains(string toml, string id) =>
        Regex.Matches(toml, "^id = \"([^\"]+)\"\\r?$", RegexOptions.Multiline)
            .Any(m => string.Equals(m.Groups[1].Value, id, StringComparison.Ordinal));

    public static (int Width, int Height) PngSize(ReadOnlySpan<byte> bytes)
    {
        ReadOnlySpan<byte> signature = [137,80,78,71,13,10,26,10];
        if (bytes.Length < 33 || !bytes[..8].SequenceEqual(signature) ||
            BinaryPrimitives.ReadUInt32BigEndian(bytes[8..12]) != 13 ||
            !bytes[12..16].SequenceEqual("IHDR"u8)) throw new InvalidDataException("Output is not PNG with IHDR.");
        var width = BinaryPrimitives.ReadInt32BigEndian(bytes[16..20]);
        var height = BinaryPrimitives.ReadInt32BigEndian(bytes[20..24]);
        if (width < 1 || height < 1) throw new InvalidDataException("PNG dimensions are invalid.");
        return (width, height);
    }

    public static void RequirePngSize(ReadOnlySpan<byte> bytes, int width, int height)
    {
        if (PngSize(bytes) != (width, height)) throw new InvalidDataException($"Output PNG size does not match {width}x{height}.");
    }

    public static void RequireCaptureResult(int width, int height, int layers, int expectedWidth, int expectedHeight)
    {
        if (width != expectedWidth || height != expectedHeight || layers != 1)
            throw new InvalidDataException("Captured document size or layer count does not match input.");
    }

    public static void RequireUnchangedHash(string before, string after)
    {
        if (!string.Equals(before, after, StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("Input PNG changed during capture.");
    }

    public static void RequireSingleMatch(int count, string name)
    {
        if (count > 1) throw new InvalidDataException($"More than one menu item named '{name}' in selected branch.");
    }

    public static void RequirePin(string version, string sha256, string expectedVersion, string expectedSha256)
    {
        if (version != expectedVersion || !string.Equals(sha256, expectedSha256, StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("Pinned paint.net version or executable SHA-256 differs from expected values.");
    }

    public static void RequireSettings(IReadOnlyDictionary<string,double> requested, IReadOnlyDictionary<string,double> actual)
    {
        foreach (var (label, value) in requested)
            if (!actual.TryGetValue(label, out var got) || got != value)
                throw new InvalidDataException($"Number box '{label}' changed after all settings were entered.");
    }

    public static bool ProcessIsolated(uint started, IEnumerable<uint> paintPids, uint windowOwner) =>
        windowOwner == started && paintPids.SequenceEqual([started]);

    /// <summary>Every entry that was added, removed or edited, by name.</summary>
    public static List<string> ChangedEntries(IReadOnlyDictionary<string,string> before, IReadOnlyDictionary<string,string> after) =>
        before.Where(p => !after.TryGetValue(p.Key, out var v) || v != p.Value).Select(p => p.Key)
            .Concat(after.Keys.Where(k => !before.ContainsKey(k))).ToList();

    /// <summary>
    /// paint.net keeps each session's scratch data in SessionData\&lt;number&gt;, under the user's folder,
    /// whatever LOCALAPPDATA says, and a stopped process leaves it behind. A file belongs to the run
    /// only if it is new and its whole session folder is new. Nothing else may be removed.
    /// </summary>
    public static List<string> RunSessionFiles(IReadOnlyDictionary<string,string> before, IReadOnlyDictionary<string,string> after)
    {
        static string? SessionFolder(string entry)
        {
            var parts = entry.Split(Path.DirectorySeparatorChar);
            return parts.Length >= 2 && parts[0].Equals("SessionData", StringComparison.OrdinalIgnoreCase) ? parts[1] : null;
        }
        var known = before.Keys.Select(SessionFolder).OfType<string>().ToHashSet(StringComparer.OrdinalIgnoreCase);
        return after.Where(p => !before.ContainsKey(p.Key) && SessionFolder(p.Key) is { } folder && !known.Contains(folder) &&
                (p.Value == "dir" || p.Key.Split(Path.DirectorySeparatorChar).Length >= 3))
            .Select(p => p.Key).ToList();
    }

    /// <summary>Deletes the named files, then any session folders that are left empty. paint.net marks its files read-only.</summary>
    public static void RemoveRunFiles(string folder, IEnumerable<string> files)
    {
        var sessionData = Path.Combine(folder, "SessionData") + Path.DirectorySeparatorChar;
        var directories = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var file in files)
        {
            var path = Path.Combine(folder, file);
            if (!path.StartsWith(sessionData, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException($"Refusing to delete a file outside SessionData: {file}");
            if (Directory.Exists(path))
            {
                directories.Add(path);
                continue;
            }
            File.SetAttributes(path, FileAttributes.Normal);
            File.Delete(path);
            directories.Add(Path.GetDirectoryName(path)!);
        }
        foreach (var directory in directories.OrderByDescending(d => d.Length))
            for (var current = directory;
                 current.StartsWith(sessionData, StringComparison.OrdinalIgnoreCase) && Directory.Exists(current) && !Directory.EnumerateFileSystemEntries(current).Any();
                 current = Path.GetDirectoryName(current)!)
                Directory.Delete(current);
    }

    /// <summary>SHA-256 of the file's decoded pixels as straight-alpha BGRA rows. Redrawing would round low-alpha pixels, so this reads them directly.</summary>
    public static string DecodedSurfaceHash(string path)
    {
        using var bitmap = new System.Drawing.Bitmap(path);
        var data = bitmap.LockBits(new System.Drawing.Rectangle(0, 0, bitmap.Width, bitmap.Height),
            System.Drawing.Imaging.ImageLockMode.ReadOnly, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
        try
        {
            using var hash = System.Security.Cryptography.IncrementalHash.CreateHash(System.Security.Cryptography.HashAlgorithmName.SHA256);
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

    public sealed record SavedFile(string Path, byte[] Bytes, DateTime WrittenUtc);

    /// <summary>
    /// paint.net rewrites its speed cache (Optimization\Startup.N.profile) in the user's folder while it runs,
    /// whatever LOCALAPPDATA says. The cache holds no settings or pictures. The driver saves it before a run and
    /// puts it back afterwards, so the user's copy finds the same bytes and times.
    /// </summary>
    public static List<SavedFile> SaveCache(string cache) =>
        !Directory.Exists(cache) ? [] :
        Directory.GetFiles(cache).Select(f => new SavedFile(f, File.ReadAllBytes(f), File.GetLastWriteTimeUtc(f))).ToList();

    public static void RestoreCache(string cache, List<SavedFile> saved)
    {
        if (!Directory.Exists(cache)) return;
        var keep = saved.Select(f => f.Path).ToHashSet(StringComparer.OrdinalIgnoreCase);
        foreach (var file in Directory.GetFiles(cache))
            if (!keep.Contains(file)) File.Delete(file);
        foreach (var file in saved)
        {
            if (!File.Exists(file.Path) || !File.ReadAllBytes(file.Path).AsSpan().SequenceEqual(file.Bytes))
                File.WriteAllBytes(file.Path, file.Bytes);
            File.SetLastWriteTimeUtc(file.Path, file.WrittenUtc);
        }
    }

    public static void RequireBeforeDeadline(TimeSpan elapsed, TimeSpan limit)
    {
        if (elapsed >= limit) throw new TimeoutException($"Whole job exceeded {limit.TotalSeconds} seconds.");
    }

    public static bool ReadersFinishedWithin(Task readers, TimeSpan remaining) => readers.Wait(remaining);

    public static void RequirePosted(bool posted, string label)
    {
        if (!posted) throw new InvalidOperationException($"Window message to '{label}' was rejected.");
    }

    public static bool MayWriteFailure(bool resultExistedAtStart) => !resultExistedAtStart;
}
