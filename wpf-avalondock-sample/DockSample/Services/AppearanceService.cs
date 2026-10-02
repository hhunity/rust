using System.Windows;

namespace DockSample.Services;

/// <summary>見た目（リソース）を実行中に変更するサービス</summary>
public interface IAppearanceService
{
    /// <summary>本文の文字サイズを変更する。見出しなどもそれに合わせて拡縮する</summary>
    void ApplyBaseFontSize(double size);
}

/// <summary>
/// Styles/Typography.xaml の FontSize.* を差し替える。
/// スタイル側が DynamicResource で参照しているので、差し替えると画面に即反映される。
/// （ViewModel から Application.Current を直接触らないよう、サービスに分けている）
/// </summary>
public class AppearanceService : IAppearanceService
{
    public void ApplyBaseFontSize(double size)
    {
        var resources = Application.Current.Resources;
        resources["FontSize.Caption"] = size - 1;
        resources["FontSize.Body"]    = size;
        resources["FontSize.Heading"] = size + 2;
        resources["FontSize.Title"]   = size + 6;
    }
}
