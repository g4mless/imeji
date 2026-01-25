using System;
using System.Drawing;
using System.IO;
using System.Windows.Forms;
using ImejiWinForms.Controls;

namespace ImejiWinForms;

public partial class MainForm : Form
{
    private ImageViewerControl _viewer = null!;
    private string? _initialFilePath;

    public MainForm(string? initialFilePath = null)
    {
        _initialFilePath = initialFilePath;
        InitializeComponent();
        SetupViewer();
        LoadWindowState();
        LoadInitialImage();
    }

    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        
        // Enable dark mode titlebar on Windows 10/11
        Utils.WindowsThemeHelper.UseImmersiveDarkMode(Handle, true);
    }

    private void LoadWindowState()
    {
        var settings = Properties.Settings.Default;
        
        // Restore window size
        if (settings.WindowSize.Width > 0 && settings.WindowSize.Height > 0)
        {
            Size = settings.WindowSize;
        }

        // Restore window location (only if it's within screen bounds)
        if (settings.WindowLocation.X != 0 || settings.WindowLocation.Y != 0)
        {
            // Check if the saved location is visible on any screen
            var savedBounds = new Rectangle(settings.WindowLocation, settings.WindowSize);
            bool isVisible = false;
            
            foreach (var screen in Screen.AllScreens)
            {
                if (screen.WorkingArea.IntersectsWith(savedBounds))
                {
                    isVisible = true;
                    break;
                }
            }

            if (isVisible)
            {
                StartPosition = FormStartPosition.Manual;
                Location = settings.WindowLocation;
            }
        }

        // Restore window state (Normal/Maximized)
        if (settings.WindowState != FormWindowState.Minimized)
        {
            WindowState = settings.WindowState;
        }
    }

    private void SaveWindowState()
    {
        var settings = Properties.Settings.Default;

        // Save window state
        settings.WindowState = WindowState;

        // Save size and location only if not minimized or maximized
        if (WindowState == FormWindowState.Normal)
        {
            settings.WindowSize = Size;
            settings.WindowLocation = Location;
        }

        settings.Save();
    }

    private void SetupViewer()
    {
        _viewer = new ImageViewerControl
        {
            Dock = DockStyle.Fill
        };

        _viewer.FilenameChanged += Viewer_FilenameChanged;
        Controls.Add(_viewer);
    }

    private void LoadInitialImage()
    {
        if (!string.IsNullOrWhiteSpace(_initialFilePath) && File.Exists(_initialFilePath))
        {
            _viewer.LoadImage(_initialFilePath);
        }
    }

    private void Viewer_FilenameChanged(object? sender, string? filename)
    {
        Text = filename ?? "Imeji";
    }

    protected override void OnDragEnter(DragEventArgs e)
    {
        base.OnDragEnter(e);

        if (e.Data?.GetDataPresent(DataFormats.FileDrop) == true)
        {
            e.Effect = DragDropEffects.Copy;
        }
    }

    protected override void OnDragDrop(DragEventArgs e)
    {
        base.OnDragDrop(e);

        if (e.Data?.GetData(DataFormats.FileDrop) is string[] files && files.Length > 0)
        {
            _viewer.LoadImage(files[0]);
        }
    }

    protected override bool ProcessCmdKey(ref Message msg, Keys keyData)
    {
        // Ctrl+W: Close current image
        if (keyData == (Keys.Control | Keys.W))
        {
            _viewer.CloseImage();
            return true;
        }

        return base.ProcessCmdKey(ref msg, keyData);
    }

    protected override void OnFormClosing(FormClosingEventArgs e)
    {
        base.OnFormClosing(e);
        SaveWindowState();
    }

}
