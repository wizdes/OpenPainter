// The harness's fixed pan path, in window pixels from the drag's start. Each axis runs a
// triangle wave, so the pointer never stops: a still pointer asks for no new frame, and
// that would look like a stall. The two periods share no factor, so the path stays fixed
// for a given image and window and still crosses most of the image.
namespace OpenPainter.Canvas;

public sealed record PanTrace(int StartX, int StartY, int ReachX, int ReachY)
{
    public const double PeriodX = 13, PeriodY = 17;

    /// A 4-point margin keeps the view off the image's edges, where a pan changes nothing.
    public static PanTrace For(double documentWidth, double documentHeight, double viewWidth, double viewHeight, double zoom, double scale)
    {
        const double EdgeMargin = 4;
        int Reach(double document, double view)
        {
            var room = (document * zoom - view) / 2 - EdgeMargin;
            if (room < 1) throw new InvalidOperationException($"PAN_TRACE: the image is {document * zoom} points on an axis of a {view} point view, so it cannot pan");
            return (int)Math.Floor(room * scale);
        }
        return new PanTrace((int)(viewWidth * scale / 2), (int)(viewHeight * scale / 2), Reach(documentWidth, viewWidth), Reach(documentHeight, viewHeight));
    }

    /// Where the pointer is `seconds` after the drag starts.
    public (int X, int Y) At(double seconds) =>
        (StartX + (int)Math.Round(Triangle(seconds / PeriodX) * ReachX), StartY + (int)Math.Round(Triangle(seconds / PeriodY) * ReachY));

    /// 0 at phase 0, up to 1, down through 0 to -1, and back to 0 at phase 1.
    static double Triangle(double phase)
    {
        var p = phase - Math.Floor(phase);
        return p < .25 ? p * 4 : p < .75 ? 2 - p * 4 : p * 4 - 4;
    }
}
