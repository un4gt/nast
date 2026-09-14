import { useEffect, useState } from 'react';
import { useWorldStore } from './worldStore';

const POSITIONS = ['角色描述前', '角色描述后', 'AN 顶部', 'AN 底部', '@D 深度', '示例前', '示例后'];
const LOGIC = ['AND_ANY（任一次级）', 'NOT_ALL（非全部）', 'NOT_ANY（全不）', 'AND_ALL（全部次级）'];

export default function WorldEditor({ onClose }: { onClose: () => void }) {
  const {
    worlds, activeWorld, entries,
    loadWorlds, openWorld, saveWorld, createWorld, deleteWorld,
    updateEntry, addEntry, deleteEntry,
  } = useWorldStore();
  const [newName, setNewName] = useState('');

  useEffect(() => {
    loadWorlds();
  }, []);

  return (
    <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4">
      <div className="bg-surface border border-line rounded-xl w-full max-w-4xl h-[85vh] flex flex-col">
        <header className="flex items-center gap-2 px-4 py-3 border-b border-line">
          <h2 className="text-lg font-semibold mr-4">世界书</h2>
          <select
            value={activeWorld ?? ''}
            onChange={(e) => openWorld(e.target.value)}
            className="bg-raised rounded px-2 py-1 text-sm"
          >
            <option value="">选择…</option>
            {worlds.map((w) => (
              <option key={w} value={w}>{w}</option>
            ))}
          </select>
          <input
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            placeholder="新书名"
            className="bg-raised rounded px-2 py-1 text-sm w-32"
          />
          <button
            onClick={() => newName.trim() && createWorld(newName.trim())}
            className="px-2 py-1 text-sm rounded bg-raised hover:bg-line"
          >
            新建
          </button>
          {activeWorld && (
            <button
              onClick={() => deleteWorld(activeWorld)}
              className="px-2 py-1 text-sm rounded bg-raised hover:bg-line text-red-400"
            >
              删除
            </button>
          )}
          <div className="flex-1" />
          <button onClick={onClose} className="px-3 py-1 text-sm rounded bg-raised hover:bg-line">
            关闭
          </button>
        </header>

        {activeWorld && (
          <>
            <div className="flex gap-2 px-4 py-2 border-b border-line">
              <button onClick={addEntry} className="px-3 py-1 text-sm rounded bg-accent text-white">
                + 添加条目
              </button>
              <button onClick={saveWorld} className="px-3 py-1 text-sm rounded bg-emerald-600 text-white">
                保存
              </button>
            </div>
            <div className="flex-1 overflow-y-auto p-4 space-y-3">
              {Object.entries(entries).map(([uid, e]) => (
                <div key={uid} className="border border-line rounded-lg p-3 space-y-2 bg-bg">
                  <div className="flex gap-2 items-center">
                    <input
                      value={e.comment}
                      onChange={(ev) => updateEntry(uid, { comment: ev.target.value })}
                      placeholder="标题/备注"
                      className="bg-raised rounded px-2 py-1 text-sm flex-1"
                    />
                    <label className="flex items-center gap-1 text-xs text-muted">
                      <input
                        type="checkbox"
                        checked={e.disable}
                        onChange={(ev) => updateEntry(uid, { disable: ev.target.checked })}
                      />
                      禁用
                    </label>
                    <label className="flex items-center gap-1 text-xs text-muted">
                      <input
                        type="checkbox"
                        checked={e.constant}
                        onChange={(ev) => updateEntry(uid, { constant: ev.target.checked })}
                      />
                      常驻
                    </label>
                    <button
                      onClick={() => deleteEntry(uid)}
                      className="px-2 py-1 text-xs rounded bg-raised hover:bg-line text-red-400"
                    >
                      删除
                    </button>
                  </div>
                  <div className="flex gap-2">
                    <input
                      value={e.key.join(', ')}
                      onChange={(ev) =>
                        updateEntry(uid, { key: ev.target.value.split(',').map((s) => s.trim()).filter(Boolean) })
                      }
                      placeholder="主关键词（逗号分隔）"
                      className="bg-raised rounded px-2 py-1 text-sm flex-1"
                    />
                    <input
                      value={e.keysecondary.join(', ')}
                      onChange={(ev) =>
                        updateEntry(uid, {
                          keysecondary: ev.target.value.split(',').map((s) => s.trim()).filter(Boolean),
                        })
                      }
                      placeholder="次级关键词"
                      className="bg-raised rounded px-2 py-1 text-sm flex-1"
                    />
                  </div>
                  <textarea
                    value={e.content}
                    onChange={(ev) => updateEntry(uid, { content: ev.target.value })}
                    placeholder="内容（支持宏）"
                    rows={3}
                    className="bg-raised rounded px-2 py-1 text-sm w-full resize-y"
                  />
                  <div className="flex gap-3 flex-wrap text-xs text-muted items-center">
                    <label>
                      位置
                      <select
                        value={String(e.position)}
                        onChange={(ev) => updateEntry(uid, { position: Number(ev.target.value) })}
                        className="bg-raised rounded px-1 py-0.5 ml-1"
                      >
                        {POSITIONS.map((p, i) => (
                          <option key={i} value={i}>{p}</option>
                        ))}
                      </select>
                    </label>
                    {Number(e.position) === 4 && (
                      <label>
                        深度
                        <input
                          type="number"
                          value={e.depth}
                          onChange={(ev) => updateEntry(uid, { depth: Number(ev.target.value) })}
                          className="bg-raised rounded px-1 py-0.5 ml-1 w-14"
                        />
                      </label>
                    )}
                    {e.selective && e.keysecondary.length > 0 && (
                      <label>
                        次级逻辑
                        <select
                          value={e.selectiveLogic}
                          onChange={(ev) => updateEntry(uid, { selectiveLogic: Number(ev.target.value) })}
                          className="bg-raised rounded px-1 py-0.5 ml-1"
                        >
                          {LOGIC.map((l, i) => (
                            <option key={i} value={i}>{l}</option>
                          ))}
                        </select>
                      </label>
                    )}
                    <label>
                      顺序
                      <input
                        type="number"
                        value={e.order}
                        onChange={(ev) => updateEntry(uid, { order: Number(ev.target.value) })}
                        className="bg-raised rounded px-1 py-0.5 ml-1 w-16"
                      />
                    </label>
                    <label>
                      插入概率
                      <input
                        type="number"
                        value={e.probability}
                        onChange={(ev) => updateEntry(uid, { probability: Number(ev.target.value) })}
                        className="bg-raised rounded px-1 py-0.5 ml-1 w-14"
                      />
                      %
                    </label>
                    {Number(e.position) === 4 && (
                      <label>
                        角色
                        <select
                          value={e.role ?? 0}
                          onChange={(ev) => updateEntry(uid, { role: Number(ev.target.value) })}
                          className="bg-raised rounded px-1 py-0.5 ml-1"
                        >
                          <option value={0}>system</option>
                          <option value={1}>user</option>
                          <option value={2}>assistant</option>
                        </select>
                      </label>
                    )}
                    <label>
                      驻留
                      <input
                        type="number"
                        value={e.sticky}
                        onChange={(ev) => updateEntry(uid, { sticky: Number(ev.target.value) })}
                        className="bg-raised rounded px-1 py-0.5 ml-1 w-12"
                      />
                    </label>
                    <label>
                      冷却
                      <input
                        type="number"
                        value={e.cooldown}
                        onChange={(ev) => updateEntry(uid, { cooldown: Number(ev.target.value) })}
                        className="bg-raised rounded px-1 py-0.5 ml-1 w-12"
                      />
                    </label>
                    <label>
                      延迟
                      <input
                        type="number"
                        value={e.delay}
                        onChange={(ev) => updateEntry(uid, { delay: Number(ev.target.value) })}
                        className="bg-raised rounded px-1 py-0.5 ml-1 w-12"
                      />
                    </label>
                    <label className="flex items-center gap-1">
                      <input
                        type="checkbox"
                        checked={e.prevent_recursion ?? e.preventRecursion}
                        onChange={(ev) => updateEntry(uid, { preventRecursion: ev.target.checked })}
                      />
                      阻止递归
                    </label>
                    <label className="flex items-center gap-1">
                      <input
                        type="checkbox"
                        checked={e.ignoreBudget}
                        onChange={(ev) => updateEntry(uid, { ignoreBudget: ev.target.checked })}
                      />
                      忽略预算
                    </label>
                  </div>
                </div>
              ))}
              {Object.keys(entries).length === 0 && (
                <div className="text-sm text-muted text-center pt-8">此书暂无条目</div>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
