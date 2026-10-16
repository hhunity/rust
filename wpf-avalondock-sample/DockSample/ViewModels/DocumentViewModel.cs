using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace DockSample.ViewModels;

/// <summary>定型文。Text は挿入するたびに作る（日付など、その時の値を入れられるように）</summary>
public record Snippet(string Name, Func<string> Text);

/// <summary>中央のドキュメント領域に表示するペイン</summary>
public partial class DocumentViewModel : PaneViewModel
{
    public DocumentViewModel(string contentId, string title, string? buttonText = null)
        : base(contentId, title, DockLocation.Document, buttonText)
    {
    }

    /// <summary>非表示にしても ViewModel は残るので、再表示すると内容もそのまま</summary>
    [ObservableProperty] private string _text = "";

    /// <summary>「定型文 ▾」ボタンの一覧（DropDownButton の例。数が多いのでスクロール・検索になる）</summary>
    public IReadOnlyList<Snippet> Snippets => AllSnippets;

    /// <summary>選ばれた定型文を末尾に追加する</summary>
    [RelayCommand]
    private void InsertSnippet(Snippet snippet)
    {
        var text = snippet.Text();
        Text = Text.Length == 0 || Text.EndsWith('\n')
            ? Text + text
            : Text + Environment.NewLine + text;
    }

    private static readonly Snippet[] AllSnippets =
    {
        new("今日の日付",           () => DateTime.Today.ToString("yyyy/MM/dd (ddd)")),
        new("現在時刻",             () => DateTime.Now.ToString("HH:mm")),
        new("区切り線",             () => new string('-', 40)),
        new("挨拶：おはようございます", () => "おはようございます。"),
        new("挨拶：お疲れさまです",   () => "お疲れさまです。"),
        new("挨拶：お世話になっております", () => "いつもお世話になっております。"),
        new("締め：よろしくお願いします", () => "よろしくお願いいたします。"),
        new("締め：以上です",         () => "以上です。"),
        new("見出し：概要",           () => "■ 概要"),
        new("見出し：目的",           () => "■ 目的"),
        new("見出し：内容",           () => "■ 内容"),
        new("見出し：課題",           () => "■ 課題"),
        new("見出し：次回までの対応", () => "■ 次回までの対応"),
        new("見出し：備考",           () => "■ 備考"),
        new("箇条書き",               () => "・"),
        new("番号付きリスト",         () => "1. \n2. \n3. "),
        new("TODO",                   () => "[ ] "),
        new("完了済み TODO",          () => "[x] "),
        new("確認依頼",               () => "お手すきの際にご確認をお願いいたします。"),
        new("返信待ち",               () => "ご返信をお待ちしております。"),
        new("議事録の型",             () =>
            $"■ 日時\n{DateTime.Today:yyyy/MM/dd}\n\n■ 参加者\n・\n\n■ 議題\n1. \n\n■ 決定事項\n・\n\n■ 次回までの対応\n[ ] "),
        new("日報の型",               () =>
            $"■ {DateTime.Today:yyyy/MM/dd} 日報\n\n■ 今日やったこと\n・\n\n■ 明日やること\n・\n\n■ 困っていること\n・"),
        new("週報の型",               () => "■ 今週の成果\n・\n\n■ 来週の予定\n・\n\n■ 課題・相談\n・"),
    };
}
