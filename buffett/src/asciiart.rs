//! The `ascii_art` module implement fancy ascii arts
//mvp001
use std::thread::sleep;
use std::time::Duration;
pub fn welcome() {
    println!(
        "
//   /$$$$$$$  /$$$$$$ /$$$$$$$$  /$$$$$$   /$$$$$$  /$$   /$$  /$$$$$$  /$$   /$$                                         
//  | $$__  $$|_  $$_/|__  $$__/ /$$__  $$ /$$__  $$| $$$ | $$ /$$__  $$| $$  | $$                                         
//  | $$  \\ $$  | $$     | $$   | $$  \\__/| $$  \\ $$| $$$$| $$| $$  \\__/| $$  | $$                                         
//  | $$$$$$$   | $$     | $$   | $$      | $$  | $$| $$ $$ $$| $$      | $$$$$$$$                                         
//  | $$__  $$  | $$     | $$   | $$      | $$  | $$| $$  $$$$| $$      | $$__  $$                                         
//  | $$  \\ $$  | $$     | $$   | $$    $$| $$  | $$| $$\\  $$$| $$    $$| $$  | $$                                         
//  | $$$$$$$/ /$$$$$$   | $$   |  $$$$$$/|  $$$$$$/| $$ \\  $$|  $$$$$$/| $$  | $$                                         
//  |_______/ |______/   |__/    \\______/  \\______/ |__/  \\__/ \\______/ |__/  |__/                                         
//                                                                                                                         
//                                                                                                                         
//                                                                                                                         
//   /$$$$$$$  /$$$$$$$$ /$$$$$$$$  /$$$$$$        /$$   /$$ /$$$$$$$$ /$$$$$$$$ /$$      /$$  /$$$$$$  /$$$$$$$  /$$   /$$
//  | $$__  $$| $$_____/|__  $$__/ /$$__  $$      | $$$ | $$| $$_____/|__  $$__/| $$  /$ | $$ /$$__  $$| $$__  $$| $$  /$$/
//  | $$  \\ $$| $$         | $$   | $$  \\ $$      | $$$$| $$| $$         | $$   | $$ /$$$| $$| $$  \\ $$| $$  \\ $$| $$ /$$/ 
//  | $$$$$$$ | $$$$$      | $$   | $$$$$$$$      | $$ $$ $$| $$$$$      | $$   | $$/$$ $$ $$| $$  | $$| $$$$$$$/| $$$$$/  
//  | $$__  $$| $$__/      | $$   | $$__  $$      | $$  $$$$| $$__/      | $$   | $$$$_  $$$$| $$  | $$| $$__  $$| $$  $$  
//  | $$  \\ $$| $$         | $$   | $$  | $$      | $$\\  $$$| $$         | $$   | $$$/ \\  $$$| $$  | $$| $$  \\ $$| $$\\  $$ 
//  | $$$$$$$/| $$$$$$$$   | $$   | $$  | $$      | $$ \\  $$| $$$$$$$$   | $$   | $$/   \\  $$|  $$$$$$/| $$  | $$| $$ \\  $$
//  |_______/ |________/   |__/   |__/  |__/      |__/  \\__/|________/   |__/   |__/     \\__/ \\______/ |__/  |__/|__/  \\__/
//                                                                                                                         
//                                                                                                                         
//                                                                                                                         
                                                                              
"
    );
    sleep(Duration::from_millis(500));
}
