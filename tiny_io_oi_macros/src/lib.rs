use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data, Fields};

#[proc_macro_attribute]
pub fn io_oi_node(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let struct_name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields_named) => &fields_named.named,
            _ => {
                return syn::Error::new_spanned(
                    &input.ident,
                    "#[io_oi_node] only supports structs with named fields"
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(
                &input.ident,
                "#[io_oi_node] only supports structs"
            )
            .to_compile_error()
            .into();
        }
    };

    let mut pwm_fields = Vec::new();
    let mut gpio_fields = Vec::new();

    for field in fields {
        let field_name = &field.ident;
        for attr in &field.attrs {
            if attr.path().is_ident("bind") {
                let mut channel: Option<u8> = None;
                let mut strategy: Option<String> = None;

                let parse_res = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("channel") {
                        let value = meta.value()?;
                        let lit: syn::LitInt = value.parse()?;
                        channel = Some(lit.base10_parse::<u8>()?);
                    } else if meta.path.is_ident("strategy") {
                        let value = meta.value()?;
                        let lit: syn::LitStr = value.parse()?;
                        strategy = Some(lit.value());
                    }
                    Ok(())
                });

                if parse_res.is_ok() {
                    if let (Some(ch), Some(strat)) = (channel, strategy) {
                        if strat == "PWM" {
                            pwm_fields.push((field_name, ch));
                        } else if strat == "GPIO" {
                            gpio_fields.push((field_name, ch));
                        }
                    }
                }
            }
        }
    }

    // Generate Safe Shutdown tokens that force all bound PWMs to 0 speed on trap
    let mut shutdown_stmts = Vec::new();
    for (field_name, _ch) in &pwm_fields {
        shutdown_stmts.push(quote! {
            self.#field_name.set_speed(0);
        });
    }
    let safe_shutdown = quote! {
        #(#shutdown_stmts)*
    };

    // Generate PWM routing tokens supporting multiple channels via match arm
    let mut pwm_branches = Vec::new();
    for (field_name, ch) in &pwm_fields {
        pwm_branches.push(quote! {
            #ch => {
                self.#field_name.set_speed(*speed);
            }
        });
    }
    let pwm_routing = quote! {
        match *channel {
            #(#pwm_branches)*
            _ => {}
        }
    };

    // Generate GPIO routing tokens
    let mut gpio_branches = Vec::new();
    for (field_name, ch) in &gpio_fields {
        gpio_branches.push(quote! {
            if *pin == #ch {
                let actual = self.#field_name.read_pin(*pin);
                if actual != *expected {
                    #safe_shutdown
                    return Err(crate::VmError::AssertionFailed {
                        pin: *pin,
                        expected: *expected,
                        actual,
                    });
                }
            }
        });
    }

    let gpio_routing = quote! {
        #(#gpio_branches)*
    };

    // We strip the #[bind(...)] helper attribute from the struct fields
    // to prevent Rust compiler from complaining about custom attributes.
    let mut clean_input = input.clone();
    if let Data::Struct(data_struct) = &mut clean_input.data {
        if let Fields::Named(fields_named) = &mut data_struct.fields {
            for field in &mut fields_named.named {
                field.attrs.retain(|attr| !attr.path().is_ident("bind"));
            }
        }
    }

    let expanded = quote! {
        #clean_input

        impl #impl_generics #struct_name #ty_generics #where_clause {
            pub fn run_vm_script(
                &mut self,
                script: &crate::ArchivedVmScript,
                fuel: &mut u32,
            ) -> Result<(), crate::VmError> {
                for step in script.steps.iter() {
                    if *fuel == 0 {
                        #safe_shutdown
                        return Err(crate::VmError::OutOfFuel);
                    }
                    *fuel -= 1;

                    match step {
                        crate::ArchivedVmStep::SetPwm { channel, speed } => {
                            #pwm_routing
                        }
                        crate::ArchivedVmStep::Delay { ticks } => {
                            let cost = (*ticks).min(*fuel);
                            *fuel -= cost;
                        }
                        crate::ArchivedVmStep::AssertOrYield { pin, expected } => {
                            #gpio_routing
                        }
                    }
                }
                Ok(())
            }
        }
    };

    expanded.into()
}
