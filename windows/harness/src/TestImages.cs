// Test pictures the harness makes in its own temp folder, so no 24 MP file is committed.
// Each pixel's value comes from a formula, so a read-back knows what it must see anywhere.
using System.IO.Compression;

namespace OpenPainter.Harness;

public static class TestImages
{
    public const int CellSize = 128;
    public const int Width24Mp = 6000, Height24Mp = 4000;

    /// Flat cells of known colors, never near black, so a missing frame cannot pass.
    public static Rgb CellColor(int x, int y)
    {
        var column = x / CellSize;
        var row = y / CellSize;
        var hash = (uint)(column * 73856093) ^ (uint)(row * 19349663);
        hash = (hash ^ (hash >> 13)) * 1274126177;
        return new Rgb((byte)(48 + hash % 192), (byte)(48 + (hash >> 8) % 192), (byte)(48 + (hash >> 16) % 192));
    }

    /// A cell's color, or null when the point is within `margin` of the cell's edge.
    public static Rgb? CellColorInside(double docX, double docY, double margin)
    {
        var inX = docX % CellSize;
        var inY = docY % CellSize;
        if (inX < margin || inY < margin || inX > CellSize - margin || inY > CellSize - margin) return null;
        return CellColor((int)docX, (int)docY);
    }

    public static void WriteCells(string path, int width, int height) =>
        Png.Write(path, width, height, (y, row) =>
        {
            for (var x = 0; x < width; x += CellSize)
            {
                var color = CellColor(x, y);
                var end = Math.Min(x + CellSize, width);
                for (var i = x; i < end; i++) (row[i * 3], row[i * 3 + 1], row[i * 3 + 2]) = (color.R, color.G, color.B);
            }
        });

    /// The color check's patches: primaries, secondaries, a gray ramp, and colors whose
    /// channels differ, in sRGB, as a PNG with no color tag means.
    public static readonly (string Name, Rgb Color)[] Patches =
    [
        ("red", new(255, 0, 0)), ("green", new(0, 255, 0)), ("blue", new(0, 0, 255)),
        ("cyan", new(0, 255, 255)), ("magenta", new(255, 0, 255)), ("yellow", new(255, 255, 0)),
        ("black", new(0, 0, 0)), ("gray 64", new(64, 64, 64)), ("gray 128", new(128, 128, 128)),
        ("gray 192", new(192, 192, 192)), ("white", new(255, 255, 255)), ("orange", new(207, 104, 77)),
        ("skin", new(224, 172, 140)), ("sky", new(90, 150, 220)), ("leaf", new(70, 130, 60)),
        ("dark red", new(90, 16, 24)), ("near black", new(8, 12, 16)), ("near white", new(248, 244, 240)),
    ];

    public const int PatchSize = 120, PatchColumns = 6;
    public static int PatchRows => (Patches.Length + PatchColumns - 1) / PatchColumns;
    public static int PatchImageWidth => PatchColumns * PatchSize;
    public static int PatchImageHeight => PatchRows * PatchSize;

    public static Rgb PatchAt(int x, int y) => Patches[y / PatchSize * PatchColumns + x / PatchSize].Color;

    public static void WritePatches(string path) =>
        Png.Write(path, PatchImageWidth, PatchImageHeight, (y, row) =>
        {
            for (var x = 0; x < PatchImageWidth; x++)
            {
                var color = PatchAt(x, y);
                (row[x * 3], row[x * 3 + 1], row[x * 3 + 2]) = (color.R, color.G, color.B);
            }
        });
}

/// A small PNG writer: 8-bit RGB, no filter. The harness needs nothing more.
public static class Png
{
    static readonly uint[] Crc = BuildCrc();

    static uint[] BuildCrc()
    {
        var table = new uint[256];
        for (uint n = 0; n < 256; n++)
        {
            var c = n;
            for (var k = 0; k < 8; k++) c = (c & 1) != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
            table[n] = c;
        }
        return table;
    }

    static void Chunk(Stream to, string type, ReadOnlySpan<byte> data)
    {
        Span<byte> number = stackalloc byte[4];
        System.Buffers.Binary.BinaryPrimitives.WriteInt32BigEndian(number, data.Length);
        to.Write(number);
        var crc = 0xFFFFFFFFu;
        foreach (var c in type)
        {
            to.WriteByte((byte)c);
            crc = Crc[(crc ^ (byte)c) & 0xFF] ^ (crc >> 8);
        }
        to.Write(data);
        foreach (var b in data) crc = Crc[(crc ^ b) & 0xFF] ^ (crc >> 8);
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(number, crc ^ 0xFFFFFFFFu);
        to.Write(number);
    }

    /// `fill` writes one row of red, green, blue bytes.
    public static void Write(string path, int width, int height, Action<int, byte[]> fill)
    {
        using var file = File.Create(path);
        file.Write(new byte[] { 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A });
        var header = new byte[13];
        System.Buffers.Binary.BinaryPrimitives.WriteInt32BigEndian(header, width);
        System.Buffers.Binary.BinaryPrimitives.WriteInt32BigEndian(header.AsSpan(4), height);
        (header[8], header[9]) = (8, 2);   // 8 bits, RGB.
        Chunk(file, "IHDR", header);
        using var packed = new MemoryStream();
        using (var zip = new ZLibStream(packed, CompressionLevel.Fastest, leaveOpen: true))
        {
            var row = new byte[width * 3];
            for (var y = 0; y < height; y++)
            {
                fill(y, row);
                zip.WriteByte(0);
                zip.Write(row);
            }
        }
        Chunk(file, "IDAT", packed.GetBuffer().AsSpan(0, (int)packed.Length));
        Chunk(file, "IEND", []);
    }

    /// Saves a captured picture.
    public static void Write(string path, Picture picture) =>
        Write(path, picture.Width, picture.Height, (y, row) =>
        {
            for (var x = 0; x < picture.Width; x++)
            {
                var color = picture.At(x, y);
                (row[x * 3], row[x * 3 + 1], row[x * 3 + 2]) = (color.R, color.G, color.B);
            }
        });
}
