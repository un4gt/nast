# 前端页面架构

```
┌──────────────┬───────────────────────────────┬──────────────────┐
│ Characters   │ Alice                         │ Character        │
│              │                               │                  │
│ Alice        │ 头像  Alice                    │ Overview         │
│ Luna         │                               │ Persona          │
│ Miku         │  AI message...                │ World Info       │
│              │                               │ Memory           │
│──────────────│  You message...               │ Author Note      │
│ Chats        │                               │                  │
│              │                               │ Generation       │
│ Today        │                               │ Preset           │
│ Yesterday    │                               │ Samplers         │
│ ...          │                               │                  │
│              │                               │                  │
│ Settings     │ ┌───────────────────────────┐ │                  │
│              │ │ message...        🎤 📎   │ │                  │
└──────────────┴─┴───────────────────────────┴─┴──────────────────┘
```

视觉层次

```
Left Sidebar
├─ Characters
├─ Chats
└─ Groups

Center
└─ Conversation

Right Inspector
├─ Character
├─ Persona
├─ Lore
├─ Memory
└─ Generation
```

## 设计

layout:
shadcn sidebar-15

message UI:
shadcn-chat 风格

streaming / AI:
shadcn chatbot-template

RP inspector:
自己做


### 相关链接

shadcn sidebar-15 https://ui.shadcn.com/view/new-york-v4/sidebar-15

shadcn chatbot-template
