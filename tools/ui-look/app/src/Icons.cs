// Loads the project's SVG icons and draws them in a token color.
// The icons hold only <path> elements, so this small reader covers them with no extra
// package. Anything else in a file is an error, so a bad icon shows up at once.
using System.Globalization;
using System.Xml.Linq;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;

namespace UiLook;

public sealed record IconPart(Geometry Geometry, bool Fill, double StrokeWidth, double[]? Dashes);
public sealed record IconArt(double ViewBox, IReadOnlyList<IconPart> Parts);

public static class Icons
{
    static readonly Dictionary<string, IconArt> Cache = new();

    public static string Folder => Path.Combine(AppContext.BaseDirectory, "assets", "icons");

    public static IconArt Load(string name)
    {
        if (Cache.TryGetValue(name, out var art)) return art;
        art = Read(Path.Combine(Folder, name + ".svg"));
        Cache[name] = art;
        return art;
    }

    public static IconArt Read(string path)
    {
        var root = XDocument.Load(path).Root ?? throw new InvalidDataException($"{path}: no root element");
        if (root.Name.LocalName != "svg") throw new InvalidDataException($"{path}: root is {root.Name.LocalName}");
        var box = (root.Attribute("viewBox")?.Value ?? throw new InvalidDataException($"{path}: no viewBox")).Split(' ');
        if (box.Length != 4 || box[0] != "0" || box[1] != "0" || box[2] != box[3])
            throw new InvalidDataException($"{path}: viewBox must be a square from 0 0");
        var parts = new List<IconPart>();
        foreach (var el in root.Elements())
        {
            if (el.Name.LocalName != "path") throw new InvalidDataException($"{path}: unsupported element {el.Name.LocalName}");
            var d = el.Attribute("d")?.Value ?? throw new InvalidDataException($"{path}: path with no d");
            var evenOdd = el.Attribute("fill-rule")?.Value == "evenodd";
            var geometry = Geometry.Parse((evenOdd ? "F0 " : "F1 ") + d);
            var fill = el.Attribute("fill")?.Value != "none";
            var strokeWidth = el.Attribute("stroke") is null ? 0 : Num(el.Attribute("stroke-width")?.Value ?? "1");
            var dashes = el.Attribute("stroke-dasharray")?.Value.Split(' ').Select(Num).ToArray();
            parts.Add(new IconPart(geometry, fill, strokeWidth, dashes));
        }
        if (parts.Count == 0) throw new InvalidDataException($"{path}: no paths");
        return new IconArt(Num(box[2]), parts);
    }

    static double Num(string s) => double.Parse(s, CultureInfo.InvariantCulture);
}

/// One icon, drawn at any size, in one color.
public sealed class IconView : Control
{
    readonly IconArt _art;
    readonly IBrush _brush;

    public IconView(string name, Color color, double size = Tokens.IconSize)
    {
        _art = Icons.Load(name);
        _brush = new SolidColorBrush(color);
        Width = size;
        Height = size;
        IsHitTestVisible = false;
    }

    public override void Render(DrawingContext context)
    {
        var scale = Bounds.Width / _art.ViewBox;
        using (context.PushTransform(Matrix.CreateScale(scale, scale)))
        {
            foreach (var part in _art.Parts)
            {
                IPen? pen = part.StrokeWidth > 0
                    ? new Pen(_brush, part.StrokeWidth, part.Dashes is null ? null : new DashStyle(part.Dashes, 0))
                    : null;
                context.DrawGeometry(part.Fill ? _brush : null, pen, part.Geometry);
            }
        }
    }
}
