namespace DockSample.Services;

/// <summary>
/// 上限付きのスタック（Undo / Redo の履歴用）。
/// 上限を超えて積むと、一番古いもの（一番下）から捨てる。
/// Stack&lt;T&gt; は下から捨てられないので、両端から出し入れできる LinkedList で作っている。
/// </summary>
public class HistoryStack<T>
{
    private readonly LinkedList<T> _items = new();   // First = 一番上（新しい） / Last = 一番下（古い）

    public HistoryStack(int capacity) => Capacity = capacity;

    /// <summary>積める数の上限</summary>
    public int Capacity { get; }

    public int Count => _items.Count;

    public void Push(T item)
    {
        _items.AddFirst(item);
        while (_items.Count > Capacity)
            _items.RemoveLast();   // 上限を超えた分は古いものから捨てる
    }

    public bool TryPop(out T item)
    {
        if (_items.First is not { } node) { item = default!; return false; }
        _items.RemoveFirst();
        item = node.Value;
        return true;
    }

    public bool TryPeek(out T item)
    {
        if (_items.First is not { } node) { item = default!; return false; }
        item = node.Value;
        return true;
    }

    public void Clear() => _items.Clear();
}
