// Copyright (C) 2022 Kingtous
//
// This file is part of RustPlayer.
//
// RustPlayer is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// RustPlayer is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with RustPlayer.  If not, see <http://www.gnu.org/licenses/>.

use std::{
    cell::{Ref, RefCell},
    fmt::Debug,
    fs::File,
    io::{BufReader, Error},
    ops::Add,
    path::Path,
    ptr::null,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use rodio::cpal;
use rodio::{Decoder, Devices, OutputStream, OutputStreamHandle, Sink, Source};
use tui::widgets::ListState;

use crate::{app, util::lyrics::{Lyrics, Lyric}};

use super::media::Media;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum PlayStatus {
    Waiting,
    Playing(Instant, Duration), // elapsed, times already played
    Stopped(Duration),          // times already played
}

pub struct PlayListItem {
    pub name: String,
    pub duration: Duration,
    pub current_pos: Duration,
    pub status: PlayStatus,
    pub path: String,
    pub lyrics: Lyrics,
    pub lyrics_index: ListState,
}

pub struct PlayList {
    pub lists: Vec<PlayListItem>,
}

pub trait Player {
    // 初始化
    fn new() -> Self;

    // 添加歌曲
    fn add_to_list(&mut self, media: Media, once: bool) -> bool;

    // 播放
    fn play(&mut self) -> bool;

    // 下一首
    fn next(&mut self) -> bool;

    // 停止
    fn stop(&mut self) -> bool;

    // 暂停
    fn pause(&mut self) -> bool;

    // 继续
    fn resume(&mut self) -> bool;

    // 播放进度
    fn get_progress(&self) -> (f32, f32);

    // 是否正在播放
    fn is_playing(&self) -> bool;

    // 提供一个接口，用于更新player状态
    fn tick(&mut self);

    // 当前歌词
    fn current_lyric(&self) -> &str;

    // 有歌词
    fn has_lyrics(&self) -> bool;
}

pub struct MusicPlayer {
    // params
    pub current_time: Duration,
    pub total_time: Duration,
    pub play_list: PlayList,
    // media: Media,
    // stream
    stream: OutputStream,
    stream_handle: OutputStreamHandle,
    sink: Sink,
    current_lyric: Option<String>,
    initialized: bool,
}

impl Player for MusicPlayer {
    fn new() -> Self {
        for dev in cpal::available_hosts() {
            println!("{:?}", dev);
        }
        let (stream, stream_handle) = OutputStream::try_default().unwrap();
        let sink = Sink::try_new(&stream_handle).unwrap();
        Self {
            current_time: Duration::from_secs(0),
            total_time: Duration::from_secs(0),
            play_list: PlayList { lists: vec![] },
            // media: f,
            stream,
            stream_handle,
            sink,
            current_lyric: None,
            initialized: false,
        }
    }

    fn add_to_list(&mut self, media: Media, once: bool) -> bool {
        match media.src {
            super::media::Source::Http(_) => false,
            super::media::Source::Local(path) => {
                let duration: Duration;
                if path.ends_with(".mp3") {
                    let dur = mp3_duration::from_path(path.clone());
                    match dur {
                        Ok(dur) => {
                            duration = dur;
                        }
                        Err(err) => {
                            // EOF catch
                            duration = err.at_duration;
                            if duration.is_zero() {
                                return false;
                            }
                        }
                    }
                } else {
                    if let Ok(f) = File::open(path.as_str()) {
                        let dec = Decoder::new(f);
                        if let Ok(dec) = dec {
                            if let Some(dur) = dec.total_duration() {
                                duration = dur;
                            } else {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                // find lyrics
                let lyrics = Lyrics::from_music_path(path.as_str());
                // open
                match File::open(path.as_str()) {
                    Ok(f) => {
                        let path = Path::new(path.as_str());
                        let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                        // Result<(stream,streamHanlde),std::error:Error>
                        if once || self.play_list.lists.is_empty() {
                            // rebuild
                            self.stop();
                            let buf_reader = BufReader::new(f);
                            let sink = self.stream_handle.play_once(buf_reader).unwrap();
                            self.sink = sink;
                            self.play_list.lists.clear();
                        }
                        let mut state =  ListState::default();
                        state.select(Some(0));
                        self.play_list.lists.push(PlayListItem {
                            name: file_name,
                            duration: duration,
                            current_pos: Duration::from_secs(0),
                            status: PlayStatus::Waiting,
                            path: path.to_string_lossy().to_string(),
                            lyrics: lyrics,
                            lyrics_index: state,
                        });
                        if !self.initialized {
                            self.initialized = true;
                        }
                        self.play();
                        self.tick();
                        return true;
                    }
                    Err(_) => false,
                }
            }
        }
    }

    // fn next(&mut self) -> bool {
    //     // self._sink.
    //     true
    // }

    fn play(&mut self) -> bool {
        self.sink.play();
        if let Some(item) = self.play_list.lists.first_mut() {
            let status = &mut item.status;
            match status {
                PlayStatus::Waiting => {
                    *status = PlayStatus::Playing(Instant::now(), Duration::from_nanos(0));
                }
                PlayStatus::Playing(_, _) => {}
                PlayStatus::Stopped(duration) => {
                    *status = PlayStatus::Playing(Instant::now(), *duration);
                }
            }
        }
        true
    }

    fn stop(&mut self) -> bool {
        self.sink.stop();
        true
    }

    fn pause(&mut self) -> bool {
        self.sink.pause();
        if let Some(item) = self.play_list.lists.first_mut() {
            let status = &mut item.status;
            match status {
                PlayStatus::Waiting => {}
                PlayStatus::Playing(instant, duration) => {
                    *status = PlayStatus::Stopped(duration.add(instant.elapsed()));
                }
                PlayStatus::Stopped(_) => {}
            }
        }
        true
    }

    fn resume(&mut self) -> bool {
        self.sink.play();
        if let Some(item) = self.play_list.lists.first_mut() {
            let status = &mut item.status;
            match status {
                PlayStatus::Waiting => {}
                PlayStatus::Playing(_, _) => {}
                PlayStatus::Stopped(duration) => {
                    *status = PlayStatus::Playing(Instant::now(), *duration);
                }
            }
        }
        return true;
    }

    fn is_playing(&self) -> bool {
        return self.initialized && !self.sink.is_paused() && !self.play_list.lists.is_empty();
    }

    fn get_progress(&self) -> (f32, f32) {
        return (0.0, 0.0);
    }

    fn tick(&mut self) {
        let is_playing = self.is_playing();
        if let Some(song) = self.play_list.lists.first_mut() {
            let status = &mut song.status;
            match status {
                PlayStatus::Waiting => {
                    if is_playing {
                        *status = PlayStatus::Playing(Instant::now(), Duration::from_nanos(0));
                    }
                }
                PlayStatus::Playing(instant, duration) => {
                    let now = instant.elapsed().add(duration.clone());
                    if now.ge(&song.duration) {
                        // next song, delete 0
                        self.next();
                    } else {
                        // update status
                        self.current_time = now;
                        self.total_time = song.duration.clone();
                        // add lyrics
                        let selected_index = song.lyrics_index.selected().unwrap();
                        if selected_index + 1 < song.lyrics.list.len() {
                            let next_lyric= &song.lyrics.list[selected_index + 1]; 
                            if self.current_time > next_lyric.time {
                                song.lyrics_index.select(Some(selected_index + 1));
                            }
                        }
                    }
                }
                PlayStatus::Stopped(dur) => {
                    self.current_time = dur.clone();
                    self.total_time = song.duration.clone();
                }
            }
        } else {
            // stop player when no sounds
            if self.play_list.lists.is_empty() {
                self.stop();
            }
        }
    }

    fn next(&mut self) -> bool {
        let len = self.play_list.lists.len();
        if len >= 1 {
            self.play_list.lists.remove(0);
            self.stop();
            if !self.play_list.lists.is_empty() {
                // next song
                let top_music = self.play_list.lists.first().unwrap();
                let f = File::open(top_music.path.as_str()).unwrap();
                let buf_reader = BufReader::new(f);
                let (stream, stream_handle) = OutputStream::try_default().unwrap();
                self.stream = stream;
                self.stream_handle = stream_handle;
                self.sink = Sink::try_new(&self.stream_handle).unwrap();
                self.sink.append(Decoder::new(buf_reader).unwrap());
                self.play();
            }
            // for
        } else {
            // no more sound to play
            return false;
        }
        true
    }

    fn current_lyric(&self) -> &str {
        if let Some(lyric) = &self.current_lyric {
            return lyric.as_str();
        } else {
            return "No Lyrics";
        }
    }

    fn has_lyrics(&self) -> bool {
        !self.play_list.lists.is_empty() && !self.play_list.lists.first().unwrap().lyrics.list.is_empty()
    }
}

impl MusicPlayer {
    pub fn volume(&self) -> f32 {
        return self.sink.volume();
    }

    pub fn set_volume(&mut self, new_volume: f32) -> bool {
        self.sink.set_volume(new_volume);
        true
    }

    pub fn playing_song(&self) -> Option<&PlayListItem> {
        return self.play_list.lists.first();
    }
}

impl Drop for MusicPlayer {
    fn drop(&mut self) {
        // println!()
    }
}
