namespace DockSample.Services;

/// <summary>
/// 「保存先が決まっていなければ聞いてから保存する」という流れ。
/// 設定ペインの「保存」「名前を付けて保存」と、終了時の確認の両方で使う。
/// </summary>
public static class SettingsSaveFlow
{
    public const string FileFilter = "設定ファイル (*.json)|*.json|すべてのファイル (*.*)|*.*";

    /// <summary>
    /// 保存先が決まっていなければ（または askPath が true なら）保存先を聞いてから保存する。
    /// 保存できたら true。キャンセル・失敗なら false（失敗時はエラーを表示する）。
    /// </summary>
    public static bool SaveInteractive(this ISettingsService settings, IDialogService dialogs, bool askPath = false)
    {
        if (!askPath && settings.FilePath is not null)
            return ReportIfFailed(settings.Save(), settings.FilePath, dialogs);

        var path = dialogs.ShowSaveFile(
            "設定を名前を付けて保存", FileFilter,
            settings.FilePath ?? JsonSettingsService.DefaultFilePath);   // 最初に提案する場所・名前
        if (path is null) return false;   // キャンセル

        return ReportIfFailed(settings.SaveAs(path), path, dialogs);
    }

    private static bool ReportIfFailed(bool ok, string path, IDialogService dialogs)
    {
        if (!ok) dialogs.ShowError($"設定を保存できませんでした。\n{path}");
        return ok;
    }
}
