# zeditor

zeditor is a video editing tool with a tui-style interface.

The core use case is going from some source video to many new clips.

The controls will use vim-like motions to quickly navigate through frames / time

UI is mostly taken up by the preview window, which is showing the video content.
At the bottom of the screen is the control bar, the control bar has these elements,
arranged left to right in a row.
- Clip Start Frame Thumbnail
- Clip End Frame Thumbnail

User has some clip variables to set:
- Clip Start
- Clip End
- Clip Name

And some Settings:
- Chunk frames (default 30 frames, the amount of time to jump for more significant movement in "insert" mode)

The UX loop is this: 
- Video is stopped
- User presses space bar to enter "normal mode"
- Video starts playing, user presses i where they want to insert Clip Start
  - Now pressing escape returns the user to "normal mode".
  - User can make video fast forward/backward by holding arrow keys, let's have these keys play it 2x faster.
- As user navigates in insert mode, we show the frame in the preview window.
- User finds the start frame they want, and presses "enter", setting Clip Start
    - Now pressing space bar starts the clip from the selected start frame.
    - User can update this start frame by pressing space bar on a different frame.
- User presses space bar to exit insert mode, clip starts playing from Clip Start
- User watches until the Clip End they want, pressing i when they reach it.
    - Again user can fine tune frame selection here manually until:
        - User happy, presses enter, setting Clip End.
        - Now pressing space bar starts the clip from Clip Start and plays it until Clip End.
        - Pressing enter prompts the user for a Clip Name and then the clip is saved to disk
- User is returned to the initial state, video is stopped on the frame immediately after clip End.
- User presses space bar to enter "normal mode".

in "normal" mode, the source video is playing
  - tapping "h"/"l" set video playback direction.
  - holding "h"/"l" starts playback in that direction and after 300ms or so, starts playing back at 2x speed in their respective directions.

in "insert" mode, the source video is paused and the user moves through frames manually.
  - "b"/"w" to go back or forward 1 Chunk 
  - "h"/"l" to navigate frame by frame in insert mode.
