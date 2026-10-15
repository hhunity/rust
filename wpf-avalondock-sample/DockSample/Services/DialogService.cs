using System.IO;
using System.Windows;
using Microsoft.Win32;

namespace DockSample.Services;

/// <summary>
/// フォルダ選択やメッセージのダイアログ。
/// ViewModel から直接 OpenFolderDialog / MessageBox を使わず、このサービス経由で出す（MVVM のため）。
/// </summary>
public interface IDialogService
{
    /// <summary>フォルダを選ぶダイアログ（中で「新しいフォルダー」も作れる）。キャンセルなら null</summary>
    string? ShowSelectFolder(string title, string? initialFolder = null);

    /// <summary>確認（OK / キャンセル）。OK なら true</summary>
    bool Confirm(string message);

    /// <summary>エラーの表示</summary>
    void ShowError(string message);
}

public class DialogService : IDialogService
{
    private const string Caption = "DockSample";

    private static Window? Owner => Application.Current?.MainWindow;

    public string? ShowSelectFolder(string title, string? initialFolder = null)
    {
        // .NET 8 以降の WPF 標準のフォルダ選択ダイアログ
        var dialog = new OpenFolderDialog { Title = title, Multiselect = false };

        if (!string.IsNullOrEmpty(initialFolder) && Directory.Exists(initialFolder))
        {
            // 今のフォルダの「親」を開き、今のフォルダを選んだ状態にする
            var parent = Path.GetDirectoryName(initialFolder.TrimEnd(Path.DirectorySeparatorChar));
            if (!string.IsNullOrEmpty(parent)) dialog.InitialDirectory = parent;
            dialog.FolderName = initialFolder;
        }

        var ok = Owner is { } owner ? dialog.ShowDialog(owner) : dialog.ShowDialog();
        return ok == true ? dialog.FolderName : null;
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
}
