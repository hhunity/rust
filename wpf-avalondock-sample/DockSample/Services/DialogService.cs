using System.IO;
using System.Windows;
using Microsoft.Win32;

namespace DockSample.Services;

/// <summary>
/// ファイル選択やメッセージのダイアログ。
/// ViewModel から直接 OpenFileDialog / MessageBox を使わず、このサービス経由で出す（MVVM のため）。
/// </summary>
public interface IDialogService
{
    /// <summary>「開く」ダイアログ。キャンセルなら null</summary>
    string? ShowOpenFile(string title, string filter, string? initialPath = null);

    /// <summary>「名前を付けて保存」ダイアログ。キャンセルなら null</summary>
    string? ShowSaveFile(string title, string filter, string? initialPath = null);

    /// <summary>確認（OK / キャンセル）。OK なら true</summary>
    bool Confirm(string message);

    /// <summary>エラーの表示</summary>
    void ShowError(string message);
}

public class DialogService : IDialogService
{
    private const string Caption = "DockSample";

    private static Window? Owner => Application.Current?.MainWindow;

    public string? ShowOpenFile(string title, string filter, string? initialPath = null)
    {
        var dialog = new OpenFileDialog { Title = title, Filter = filter, CheckFileExists = true };
        SetInitialPath(dialog, initialPath);
        return Show(dialog) ? dialog.FileName : null;
    }

    public string? ShowSaveFile(string title, string filter, string? initialPath = null)
    {
        var dialog = new SaveFileDialog
        {
            Title = title,
            Filter = filter,
            OverwritePrompt = true,   // 既存のファイルを選んだら上書き確認
            AddExtension = true,
            DefaultExt = ".json",
        };
        SetInitialPath(dialog, initialPath);
        return Show(dialog) ? dialog.FileName : null;
    }

    public bool Confirm(string message) =>
        (Owner is { } owner
            ? MessageBox.Show(owner, message, Caption, MessageBoxButton.OKCancel, MessageBoxImage.Warning)
            : MessageBox.Show(message, Caption, MessageBoxButton.OKCancel, MessageBoxImage.Warning))
        == MessageBoxResult.OK;

    public void ShowError(string message)
    {
        if (Owner is { } owner)
            MessageBox.Show(owner, message, Caption, MessageBoxButton.OK, MessageBoxImage.Error);
        else
            MessageBox.Show(message, Caption, MessageBoxButton.OK, MessageBoxImage.Error);
    }

    private static bool Show(FileDialog dialog) =>
        (Owner is { } owner ? dialog.ShowDialog(owner) : dialog.ShowDialog()) == true;

    /// <summary>今のファイルのフォルダ・ファイル名を最初から選んだ状態にする</summary>
    private static void SetInitialPath(FileDialog dialog, string? path)
    {
        if (string.IsNullOrEmpty(path)) return;
        var dir = Path.GetDirectoryName(path);
        if (!string.IsNullOrEmpty(dir) && Directory.Exists(dir))
            dialog.InitialDirectory = dir;
        dialog.FileName = Path.GetFileName(path);
    }
}
