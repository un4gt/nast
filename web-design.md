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
