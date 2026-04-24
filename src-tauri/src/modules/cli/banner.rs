//! Terminal branding — ASCII welcome shown when entering the interactive CLI.

/// Shown above the REPL prompt (bare `pengine` in a TTY).
pub const CLI_WELCOME: &str = concat!(
    r"                                          
               :; ;;                    
                ;;;;; ;                 
                 ;;;;;;;;               
             ;;;;;;;;,;;;;              
            ;•◘◘◘○◘◘t;○◘;;;;            
           ;I;;;;;::;;;;▒:;;;           
           ;;;;;;;;;;;;;◘◘;;;           
           ;◘;▓▓▓▓▓,;◘◘◘◘•;;;           
           ~,◘◘;;:;○◘◘◘◘◘;;;            
           ;;;•◘◘◘○◘◘◘;;;;;;;.          
         ;;I;◘◘◘;;♣◘◘◘◘:;iI;;;;         
        ;;;::•;◘;;◘.◘;○;I:;;;;;;        
       ;;;;II;◘◘;;◘◘○I;II;;;;;;;;       
       ;;;;;I;◘;;;◘◘;II;;;;;l;;;;       
       ;;;;;;;;;;;:I;;;;;;;;I;;;;       
        ;;W;;;;;;;;;;;;;;;;;;;W,;       
         WWWWWW▓;;WW;;WW:▓WWWWW~        
         %;;W,,W:WWMWW;;WW;;& ;         
           ;; ;  ;W;;! ;W;;             
                 ;W;    ;               
",
    "\n\nPengine CLI — type /help for slash commands.\n",
);

#[cfg(test)]
mod tests {
    use super::CLI_WELCOME;

    #[test]
    fn welcome_has_brand_and_hints() {
        assert!(CLI_WELCOME.contains("Pengine CLI"));
        assert!(CLI_WELCOME.contains("/help"));
    }
}
