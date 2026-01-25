using SkiaSharp;

namespace ImejiWinForms.Utils;


public static class AnimationHelper
{

    public static float EaseOutCubic(float t)
    {
        return 1 - MathF.Pow(1 - t, 3);
    }


    public static SKPoint Lerp(SKPoint start, SKPoint end, float t)
    {
        return new SKPoint(
            start.X + (end.X - start.X) * t,
            start.Y + (end.Y - start.Y) * t
        );
    }


    public static float Lerp(float start, float end, float t)
    {
        return start + (end - start) * t;
    }
}
