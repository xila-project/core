use quote::ToTokens;
use syn::{ForeignItemFn, ItemFn, Signature, visit::Visit};

#[derive(Default)]
pub struct LvglContext {
    signatures: Vec<Signature>,
    definitions: Vec<ItemFn>,
    function_filtering: Option<fn(&Signature) -> bool>,
}

impl LvglContext {
    pub fn parse(source: &str) -> syn::Result<Self> {
        let mut context = Self::default();
        context.set_function_filtering(Some(Self::filter_function));
        context.visit_file(&syn::parse_file(source)?);
        context.set_function_filtering(None);
        context.visit_file(&syn::parse2(crate::additional::get())?);
        Ok(context)
    }

    pub fn set_function_filtering(&mut self, function_filtering: Option<fn(&Signature) -> bool>) {
        self.function_filtering = function_filtering;
    }

    pub fn get_signatures(&self) -> Vec<Signature> {
        self.signatures.clone()
    }

    pub fn get_definitions(&self) -> Vec<ItemFn> {
        self.definitions.clone()
    }

    fn contains_excluded_type(signature: &Signature) -> bool {
        signature.inputs.iter().any(|input| match input {
            syn::FnArg::Typed(pat_type) => match &*pat_type.ty {
                syn::Type::Path(type_path) => {
                    let path = type_path.path.to_token_stream().to_string();

                    path.contains("_cb_")
                }
                syn::Type::Ptr(type_ptr) => {
                    let element = type_ptr.elem.to_token_stream().to_string();

                    element.contains("lv_event_t")
                        || element.ends_with("_dsc_t")
                        || element.ends_with("_cursor_t")
                        || element.ends_with("_font_t")
                        || element.ends_with("_group_t")
                        || element.ends_with("_layer_t")
                }
                _ => false,
            },
            _ => false,
        }) || match &signature.output {
            syn::ReturnType::Type(_, type_value) => match &**type_value {
                syn::Type::Path(type_path) => {
                    let path = type_path.path.to_token_stream().to_string();

                    path.contains("_cb_")
                }
                syn::Type::Ptr(type_ptr) => {
                    let element = type_ptr.elem.to_token_stream().to_string();

                    element.contains("lv_event_t")
                        || element.ends_with("_dsc_t")
                        || element.ends_with("_cursor_t")
                        || element.ends_with("_font_t")
                        || element.ends_with("_group_t")
                        || element.ends_with("_layer_t")
                }
                _ => false,
            },
            _ => false,
        }
    }

    pub fn filter_function(signature: &Signature) -> bool {
        let unauthorized_functions = [
            "lv_obj_delete",
            "lv_obj_get_display",
            "lv_obj_delete_delayed",
            "lv_obj_delete_async",
            "lv_obj_null_on_delete",
            "lv_buttonmatrix_get_map",
            "lv_textarea_get_text",
            "lv_obj_get_layer_type",
            // Internal/private object functions are visible because private LVGL API is
            // enabled for Core, but are not safe to expose through the wasm ABI.
            "lv_obj_allocate_spec_attr",
            "lv_obj_destruct",
            "lv_obj_add_child",
            "lv_obj_remove_child",
            "lv_obj_invalidate_expand_blur",
            // LVGL's variadic API uses va_list, which cannot cross this ABI.
            "lv_label_set_text_vfmt",
            // These APIs use LVGL types not available in the wasm translation layer.
            "lv_style_set_image_colorkey",
            "lv_obj_get_style_image_colorkey",
            "lv_obj_set_style_image_colorkey",
            "lv_obj_add_play_timeline_event",
        ];

        let signature_ident_str = signature.ident.to_string();

        if unauthorized_functions.contains(&signature_ident_str.as_str()) {
            return false;
        }

        let authorized_prefixes = [
            "lv_point_",
            "lv_color",
            "lv_label",
            "lv_style_",
            "lv_obj_",
            "lv_style_",
            "lv_arc_",
            "lv_coord_",
            "lv_button_",
            "lv_buttonmatrix_",
            "lv_calendar_",
            "lv_chart_",
            "lv_checkbox_",
            "lv_dropdown_",
            "lv_led_",
            "lv_line_",
            "lv_menu_",
            "lv_msgbox_",
            "lv_roller_",
            "lv_scale_",
            "lv_slider_",
            "lv_textarea_",
            "lv_spinbox_",
            "lv_table_",
            "lv_tabview_",
            "lv_tileview_",
            "lv_subject_",
            "lv_screen_",
            "Window_",
            "Object_",
        ];

        if !authorized_prefixes
            .iter()
            .any(|&prefix| signature_ident_str.starts_with(prefix))
        {
            return false;
        }

        // Check if on of the function parameter contains a function pointer
        if Self::contains_excluded_type(signature) {
            return false;
        }

        true
    }
}

impl Visit<'_> for LvglContext {
    fn visit_foreign_item_fn(&mut self, foreign_item_function: &ForeignItemFn) {
        if let Some(filter_function) = self.function_filtering
            && !filter_function(&foreign_item_function.sig)
        {
            return;
        }

        self.signatures.push(foreign_item_function.sig.clone());
    }

    fn visit_item_fn(&mut self, i: &'_ syn::ItemFn) {
        if let Some(filter_function) = self.function_filtering
            && !filter_function(&i.sig)
        {
            return;
        }

        self.signatures.push(i.sig.clone());
        self.definitions.push(i.clone());
    }
}
