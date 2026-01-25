using SkiaSharp;
using SkiaSharp.Views.Desktop;
using System;
using System.Windows.Forms;
using ImejiWinForms.Models;
using ImejiWinForms.Utils;

namespace ImejiWinForms.Controls;


public class ImageViewerControl : SKControl
{
    private readonly ImageState _state = new();
    private readonly System.Windows.Forms.Timer _animationTimer;
    
    private const float MinZoom = 1.0f;
    private const float MaxZoom = 10.0f;
    private const float AnimationDuration = 0.3f; // 300ms
    
    public event EventHandler<string?>? FilenameChanged;

    public ImageViewerControl()
    {
        DoubleBuffered = true;
        
        // Setup animation timer (60 FPS)
        _animationTimer = new System.Windows.Forms.Timer
        {
            Interval = 16 // ~60 FPS
        };
        _animationTimer.Tick += AnimationTimer_Tick;
    }


    public void LoadImage(string filePath)
    {
        var (bitmap, error) = Services.ImageLoader.LoadFromFile(filePath);
        
        if (error != null)
        {
            MessageBox.Show(error, "Error Loading Image", MessageBoxButtons.OK, MessageBoxIcon.Error);
            return;
        }

        SetImage(bitmap, Path.GetFileName(filePath));
    }


    public void LoadImage(byte[] bytes, string? filename = null)
    {
        var (bitmap, error) = Services.ImageLoader.LoadFromBytes(bytes);
        
        if (error != null)
        {
            MessageBox.Show(error, "Error Loading Image", MessageBoxButtons.OK, MessageBoxIcon.Error);
            return;
        }

        SetImage(bitmap, filename);
    }


    public void CloseImage()
    {
        _state.Reset();
        FilenameChanged?.Invoke(this, null);
        Invalidate();
    }


    public string? GetFilename() => _state.Filename;

    private void SetImage(SKBitmap? bitmap, string? filename)
    {
        _state.Reset();
        _state.Image = bitmap;
        _state.Filename = filename;
        FilenameChanged?.Invoke(this, filename);
        Invalidate();
    }

    protected override void OnPaintSurface(SKPaintSurfaceEventArgs e)
    {
        base.OnPaintSurface(e);

        var canvas = e.Surface.Canvas;
        canvas.Clear(SKColors.Black);

        if (_state.Image == null)
            return;

        var imageSize = new SKSize(_state.Image.Width, _state.Image.Height);
        var availableSize = new SKSize(Width, Height);

        // Calculate base scale to fit image in window
        var baseScale = Math.Min(
            availableSize.Width / imageSize.Width,
            availableSize.Height / imageSize.Height
        );
        baseScale = Math.Min(baseScale, 1.0f); // Don't upscale beyond original size

        // Apply zoom
        var displaySize = new SKSize(
            imageSize.Width * baseScale * _state.Zoom,
            imageSize.Height * baseScale * _state.Zoom
        );

        // Calculate center position with pan offset
        var center = new SKPoint(availableSize.Width / 2, availableSize.Height / 2);
        var imagePos = new SKPoint(
            center.X - displaySize.Width / 2 + _state.PanOffset.X,
            center.Y - displaySize.Height / 2 + _state.PanOffset.Y
        );

        var destRect = new SKRect(imagePos.X, imagePos.Y, imagePos.X + displaySize.Width, imagePos.Y + displaySize.Height);

        var paint = new SKPaint
        {
            FilterQuality = SKFilterQuality.High,
            IsAntialias = true,
            IsDither = false
        };

        canvas.DrawBitmap(_state.Image, destRect, paint);
    }

    protected override void OnMouseWheel(MouseEventArgs e)
    {
        base.OnMouseWheel(e);

        if (_state.Image == null)
            return;

        // Calculate zoom factor from scroll delta
        var zoomFactor = 1.0f + (e.Delta / 1200.0f);
        var oldZoom = _state.Zoom;
        _state.Zoom = Math.Clamp(_state.Zoom * zoomFactor, MinZoom, MaxZoom);

        // Start animation to center when zoom reaches 1.0
        if (_state.Zoom == MinZoom && oldZoom > MinZoom)
        {
            _state.IsAnimating = true;
            _state.AnimationStartOffset = _state.PanOffset;
            _state.AnimationStartTime = DateTime.Now;
            _animationTimer.Start();
        }
        else if (_state.Zoom > MinZoom)
        {
            // Stop animation if zooming back in
            _state.IsAnimating = false;
            _animationTimer.Stop();

            // Zoom towards mouse cursor position
            var mousePos = PointToClient(Cursor.Position);
            var center = new SKPoint(Width / 2f, Height / 2f);
            var mouseOffset = new SKPoint(mousePos.X - center.X, mousePos.Y - center.Y);
            var zoomChange = _state.Zoom / oldZoom - 1.0f;
            
            _state.PanOffset = new SKPoint(
                _state.PanOffset.X - mouseOffset.X * zoomChange,
                _state.PanOffset.Y - mouseOffset.Y * zoomChange
            );
        }

        Invalidate();
    }

    protected override void OnMouseDown(MouseEventArgs e)
    {
        base.OnMouseDown(e);

        if (e.Button == MouseButtons.Left && _state.Image != null && _state.Zoom > MinZoom)
        {
            _state.IsDragging = true;
            _state.LastMousePos = new SKPoint(e.X, e.Y);
            Cursor = Cursors.Hand;
        }
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);

        if (_state.IsDragging && _state.Image != null)
        {
            var currentPos = new SKPoint(e.X, e.Y);
            var delta = new SKPoint(
                currentPos.X - _state.LastMousePos.X,
                currentPos.Y - _state.LastMousePos.Y
            );

            _state.PanOffset = new SKPoint(
                _state.PanOffset.X + delta.X,
                _state.PanOffset.Y + delta.Y
            );

            _state.LastMousePos = currentPos;
            Invalidate();
        }
    }

    protected override void OnMouseUp(MouseEventArgs e)
    {
        base.OnMouseUp(e);

        if (e.Button == MouseButtons.Left)
        {
            _state.IsDragging = false;
            Cursor = Cursors.Default;
        }
    }

    protected override void OnResize(EventArgs e)
    {
        base.OnResize(e);
        
        // Reset zoom and pan on significant window size changes
        if (_state.Image != null && _state.Zoom == MinZoom)
        {
            _state.PanOffset = SKPoint.Empty;
        }
        
        Invalidate();
    }

    private void AnimationTimer_Tick(object? sender, EventArgs e)
    {
        if (!_state.IsAnimating)
        {
            _animationTimer.Stop();
            return;
        }

        var elapsed = (float)(DateTime.Now - _state.AnimationStartTime).TotalSeconds;
        
        if (elapsed >= AnimationDuration)
        {
            // Animation complete
            _state.PanOffset = SKPoint.Empty;
            _state.IsAnimating = false;
            _animationTimer.Stop();
        }
        else
        {
            // Smooth easing function (ease-out cubic)
            var t = elapsed / AnimationDuration;
            var easedT = AnimationHelper.EaseOutCubic(t);

            // Interpolate from start offset to zero
            _state.PanOffset = AnimationHelper.Lerp(_state.AnimationStartOffset, SKPoint.Empty, easedT);
        }

        Invalidate();
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _animationTimer?.Dispose();
            _state.Image?.Dispose();
        }
        base.Dispose(disposing);
    }
}
