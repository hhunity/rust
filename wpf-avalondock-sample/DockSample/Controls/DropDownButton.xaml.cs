using System.Collections;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Threading;

namespace DockSample.Controls;

/// <summary>
/// 押すと一覧が開き、選ぶとコマンドを実行するボタン。
/// ・一覧は MaxDropDownHeight を超えるとスクロールする
/// ・検索欄に文字を打つと、表示名（DisplayMemberPath、なければ ToString()）に含むものだけに絞り込む
/// ・キーボード：↑↓で移動、Enter で決定、Esc で閉じる。マウスはクリックで決定
/// ・選んだ項目は Command の引数として渡され、ItemSelected イベントも発生する
/// </summary>
public partial class DropDownButton : UserControl
{
    /// <summary>一覧の表示用（絞り込みのため、ItemsSource とは別に自前のビューを持つ）</summary>
    private ListCollectionView? _view;

    public DropDownButton()
    {
        InitializeComponent();

        PART_Popup.Opened += OnPopupOpened;
        // 開いている間はボタンがクリックを受けないようにしておき、閉じたら戻す。
        // （開いている時にボタンを押すと「外のクリックで閉じる → ボタンで開く」が同時に起き、
        //   閉じたつもりがまた開いてしまうのを防ぐ）
        PART_Popup.Closed += (_, _) => PART_Toggle.IsHitTestVisible = true;

        PART_Search.TextChanged += (_, _) => ApplyFilter();
        PART_Search.PreviewKeyDown += OnNavigationKeyDown;
        PART_List.PreviewKeyDown += OnNavigationKeyDown;

        ApplySearchVisibility();
        ApplyDropDownSize();
    }

    /// <summary>項目が選ばれた（Command とは別に、コードビハインドで受け取りたいとき用）</summary>
    public event EventHandler<object>? ItemSelected;

    // ===== 公開プロパティ =====

    /// <summary>ボタンに表示する内容（文字など）</summary>
    public object? Header
    {
        get => GetValue(HeaderProperty);
        set => SetValue(HeaderProperty, value);
    }
    public static readonly DependencyProperty HeaderProperty = DependencyProperty.Register(
        nameof(Header), typeof(object), typeof(DropDownButton),
        new PropertyMetadata(null, (d, e) => ((DropDownButton)d).PART_Header.Content = e.NewValue));

    /// <summary>一覧に並べる項目</summary>
    public IEnumerable? ItemsSource
    {
        get => (IEnumerable?)GetValue(ItemsSourceProperty);
        set => SetValue(ItemsSourceProperty, value);
    }
    public static readonly DependencyProperty ItemsSourceProperty = DependencyProperty.Register(
        nameof(ItemsSource), typeof(IEnumerable), typeof(DropDownButton),
        new PropertyMetadata(null, (d, _) => ((DropDownButton)d).RebuildView()));

    /// <summary>各項目の表示に使うプロパティ名（検索もこの文字で行う）</summary>
    public string? DisplayMemberPath
    {
        get => (string?)GetValue(DisplayMemberPathProperty);
        set => SetValue(DisplayMemberPathProperty, value);
    }
    public static readonly DependencyProperty DisplayMemberPathProperty = DependencyProperty.Register(
        nameof(DisplayMemberPath), typeof(string), typeof(DropDownButton),
        new PropertyMetadata(null, (d, e) =>
        {
            var c = (DropDownButton)d;
            c.PART_List.DisplayMemberPath = (string?)e.NewValue;
            c.ApplyFilter();
        }));

    /// <summary>各項目の見た目（DisplayMemberPath の代わりに、凝った表示をしたいとき）</summary>
    public DataTemplate? ItemTemplate
    {
        get => (DataTemplate?)GetValue(ItemTemplateProperty);
        set => SetValue(ItemTemplateProperty, value);
    }
    public static readonly DependencyProperty ItemTemplateProperty = DependencyProperty.Register(
        nameof(ItemTemplate), typeof(DataTemplate), typeof(DropDownButton),
        new PropertyMetadata(null, (d, e) => ((DropDownButton)d).PART_List.ItemTemplate = (DataTemplate?)e.NewValue));

    /// <summary>項目が選ばれたら実行するコマンド。引数に選んだ項目が渡される</summary>
    public ICommand? Command
    {
        get => (ICommand?)GetValue(CommandProperty);
        set => SetValue(CommandProperty, value);
    }
    public static readonly DependencyProperty CommandProperty = DependencyProperty.Register(
        nameof(Command), typeof(ICommand), typeof(DropDownButton));

    /// <summary>一覧の最大の高さ。超えたらスクロール</summary>
    public double MaxDropDownHeight
    {
        get => (double)GetValue(MaxDropDownHeightProperty);
        set => SetValue(MaxDropDownHeightProperty, value);
    }
    public static readonly DependencyProperty MaxDropDownHeightProperty = DependencyProperty.Register(
        nameof(MaxDropDownHeight), typeof(double), typeof(DropDownButton),
        new PropertyMetadata(250.0, (d, _) => ((DropDownButton)d).ApplyDropDownSize()));

    /// <summary>一覧の最小の幅</summary>
    public double MinDropDownWidth
    {
        get => (double)GetValue(MinDropDownWidthProperty);
        set => SetValue(MinDropDownWidthProperty, value);
    }
    public static readonly DependencyProperty MinDropDownWidthProperty = DependencyProperty.Register(
        nameof(MinDropDownWidth), typeof(double), typeof(DropDownButton),
        new PropertyMetadata(200.0, (d, _) => ((DropDownButton)d).ApplyDropDownSize()));

    /// <summary>検索欄を出すか</summary>
    public bool IsSearchEnabled
    {
        get => (bool)GetValue(IsSearchEnabledProperty);
        set => SetValue(IsSearchEnabledProperty, value);
    }
    public static readonly DependencyProperty IsSearchEnabledProperty = DependencyProperty.Register(
        nameof(IsSearchEnabled), typeof(bool), typeof(DropDownButton),
        new PropertyMetadata(true, (d, _) => ((DropDownButton)d).ApplySearchVisibility()));

    // ===== 一覧・絞り込み =====

    private void RebuildView()
    {
        // ObservableCollection などの IList ならそのまま包む（項目の追加・削除も反映される）
        _view = ItemsSource switch
        {
            null => null,
            IList list => new ListCollectionView(list),
            var items => new ListCollectionView(items.Cast<object>().ToList()),
        };
        if (_view is not null) _view.Filter = Matches;
        PART_List.ItemsSource = _view;
        UpdateEmptyText();
    }

    private bool Matches(object item)
    {
        var query = PART_Search.Text.Trim();
        return query.Length == 0 ||
               GetDisplayText(item).Contains(query, StringComparison.CurrentCultureIgnoreCase);
    }

    private string GetDisplayText(object item)
    {
        if (string.IsNullOrEmpty(DisplayMemberPath)) return item.ToString() ?? "";
        // 単純なプロパティ名だけに対応（"A.B" のような入れ子のパスは未対応）
        return item.GetType().GetProperty(DisplayMemberPath)?.GetValue(item)?.ToString() ?? "";
    }

    private void ApplyFilter()
    {
        PART_Placeholder.Visibility = PART_Search.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        _view?.Refresh();
        SelectIndex(0);
        UpdateEmptyText();
    }

    private void UpdateEmptyText() =>
        PART_Empty.Visibility = _view is { IsEmpty: true } ? Visibility.Visible : Visibility.Collapsed;

    // ===== 開く・選ぶ・閉じる =====

    private void OnPopupOpened(object? sender, EventArgs e)
    {
        PART_Toggle.IsHitTestVisible = false;
        PART_Search.Text = "";   // 開くたびに検索をクリア（TextChanged で一覧も更新される）
        ApplyFilter();

        // 開いた直後にキーボードで操作できるよう、検索欄（なければ一覧）にフォーカスを移す
        Dispatcher.BeginInvoke(DispatcherPriority.Input, () =>
        {
            if (IsSearchEnabled) Keyboard.Focus(PART_Search);
            else Keyboard.Focus(PART_List);
        });
    }

    private void OnNavigationKeyDown(object sender, KeyEventArgs e)
    {
        switch (e.Key)
        {
            case Key.Down:   SelectIndex(PART_List.SelectedIndex + 1); e.Handled = true; break;
            case Key.Up:     SelectIndex(PART_List.SelectedIndex - 1); e.Handled = true; break;
            case Key.Enter:
                if (PART_List.SelectedItem is { } item) Commit(item);
                e.Handled = true;
                break;
            case Key.Escape: Close(); e.Handled = true; break;
        }
    }

    private void OnItemClick(object sender, MouseButtonEventArgs e)
    {
        if (sender is ListBoxItem { DataContext: { } item }) Commit(item);
    }

    private void SelectIndex(int index)
    {
        var count = PART_List.Items.Count;
        if (count == 0) { PART_List.SelectedIndex = -1; return; }
        PART_List.SelectedIndex = Math.Clamp(index, 0, count - 1);
        PART_List.ScrollIntoView(PART_List.SelectedItem);
    }

    /// <summary>選択を確定：閉じてからコマンドを実行する</summary>
    private void Commit(object item)
    {
        Close();
        if (Command is { } command && command.CanExecute(item))
            command.Execute(item);
        ItemSelected?.Invoke(this, item);
    }

    private void Close()
    {
        PART_Toggle.IsChecked = false;
        PART_Toggle.Focus();   // キーボードで続けて操作できるよう、ボタンにフォーカスを戻す
    }

    // ===== 見た目の反映 =====

    private void ApplySearchVisibility() =>
        PART_SearchArea.Visibility = IsSearchEnabled ? Visibility.Visible : Visibility.Collapsed;

    private void ApplyDropDownSize()
    {
        PART_List.MaxHeight = MaxDropDownHeight;
        PART_Border.MinWidth = MinDropDownWidth;
    }
}
