using System;
using System.Windows.Forms;

namespace ImejiWinForms;

static class Program
{

    [STAThread]
    static void Main(string[] args)
    {
        ApplicationConfiguration.Initialize();
        
        // Get file path from command line arguments if provided
        string? initialFilePath = null;
        if (args.Length > 0 && !string.IsNullOrWhiteSpace(args[0]))
        {
            initialFilePath = args[0];
        }
        
        Application.Run(new MainForm(initialFilePath));
    }
}
