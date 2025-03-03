use std::collections::{HashMap, HashSet};
use std::string::String;

use lazy_static::lazy_static;
use log::debug;
use quote::quote;
use regex::Regex;
use syn::*;

use ApiType::*;

use crate::api_types::*;
use crate::generator_rust::HANDLER_NAME;

type StructMap<'a> = HashMap<String, &'a ItemStruct>;

pub fn parse(source_rust_content: &str, file: File) -> ApiFile {
    let (src_fns, src_struct_map) = extract_items_from_file(&file);
    let parser = Parser {
        src_struct_map,
        struct_pool: HashMap::new(),
        parsing_or_parsed_struct_names: HashSet::new(),
    };
    parser.parse(source_rust_content, src_fns)
}

struct Parser<'a> {
    src_struct_map: HashMap<String, &'a ItemStruct>,
    struct_pool: ApiStructPool,
    parsing_or_parsed_struct_names: HashSet<String>,
}

fn extract_comments(attr: &Attribute) -> Option<Comment> {
    match attr.parse_meta() {
        Ok(Meta::NameValue(MetaNameValue {
            path,
            lit: Lit::Str(lit),
            ..
        })) if path.is_ident("doc") => Some(Comment::from(lit.value().as_ref())),
        _ => None,
    }
}

impl<'a> Parser<'a> {
    fn parse(mut self, source_rust_content: &str, src_fns: Vec<&ItemFn>) -> ApiFile {
        let funcs = src_fns.iter().map(|f| self.parse_function(f)).collect();

        let has_executor = source_rust_content.contains(HANDLER_NAME);

        ApiFile {
            funcs,
            struct_pool: self.struct_pool,
            has_executor,
        }
    }

    fn parse_function(&mut self, func: &ItemFn) -> ApiFunc {
        debug!("parse_function function name: {:?}", func.sig.ident);

        lazy_static! {
            static ref CAPTURE_RESULT: GenericCapture = GenericCapture::new("Result");
        }

        let sig = &func.sig;
        let func_name = ident_to_string(&sig.ident);

        let mut inputs = Vec::new();
        let mut output = None;
        let mut mode = None;

        for sig_input in &sig.inputs {
            if let FnArg::Typed(ref pat_type) = sig_input {
                let name = if let Pat::Ident(ref pat_ident) = *pat_type.pat {
                    format!("{}", pat_ident.ident)
                } else {
                    panic!("unexpected pat_type={:?}", pat_type)
                };
                let type_string = type_to_string(&pat_type.ty);

                if let Some(stream_sink_inner_type) = self.try_parse_stream_sink(&type_string) {
                    output = Some(stream_sink_inner_type);
                    mode = Some(ApiFuncMode::Stream);
                } else {
                    let comments = pat_type.attrs.iter().filter_map(extract_comments).collect();
                    inputs.push(ApiField {
                        name: ApiIdent::new(name),
                        ty: self.parse_type(&type_string),
                        comments,
                    });
                }
            } else {
                panic!("unexpected sig_input={:?}", sig_input);
            }
        }

        if output.is_none() {
            output = Some(if let ReturnType::Type(_, ty) = &sig.output {
                let type_string = type_to_string(ty);
                if let Some(inner) = CAPTURE_RESULT.captures(&type_string) {
                    self.parse_type(&inner)
                } else {
                    panic!("unsupported type_string: {}", type_string);
                }
            } else {
                panic!("unsupported output: {:?}", sig.output);
            });
            mode = Some(
                if let Some(ApiType::Delegate(ApiTypeDelegate::SyncReturnVecU8)) = output {
                    ApiFuncMode::Sync
                } else {
                    ApiFuncMode::Normal
                },
            );
        }

        let comments = func.attrs.iter().filter_map(extract_comments).collect();

        ApiFunc {
            name: func_name,
            inputs,
            output: output.expect("unsupported output"),
            mode: mode.expect("unsupported mode"),
            comments,
        }
    }

    fn parse_type(&mut self, ty: &str) -> ApiType {
        debug!("parse_type: {}", ty);
        None.or_else(|| ApiTypePrimitive::try_from_rust_str(ty).map(Primitive))
            .or_else(|| self.try_parse_api_type_delegate(ty))
            .or_else(|| self.try_parse_list(ty))
            .or_else(|| self.try_parse_box(ty))
            .or_else(|| self.try_parse_option(ty))
            .or_else(|| self.try_parse_struct(ty))
            .unwrap_or_else(|| panic!("parse_type failed for ty={}", ty))
    }

    fn try_parse_stream_sink(&mut self, ty: &str) -> Option<ApiType> {
        lazy_static! {
            static ref CAPTURE_STREAM_SINK: GenericCapture = GenericCapture::new("StreamSink");
        }

        CAPTURE_STREAM_SINK
            .captures(ty)
            .map(|inner| self.parse_type(&inner))
    }

    fn try_parse_api_type_delegate(&mut self, ty: &str) -> Option<ApiType> {
        match ty {
            "SyncReturn<Vec<u8>>" => Some(ApiType::Delegate(ApiTypeDelegate::SyncReturnVecU8)),
            "String" => Some(ApiType::Delegate(ApiTypeDelegate::String)),
            _ => {
                lazy_static! {
                    static ref CAPTURE_ZERO_COPY_BUFFER: GenericCapture =
                        GenericCapture::new("ZeroCopyBuffer");
                }

                if let Some(inner_type_str) = CAPTURE_ZERO_COPY_BUFFER.captures(ty) {
                    if let Some(ApiType::PrimitiveList(ApiTypePrimitiveList { primitive })) =
                        self.try_parse_list(&inner_type_str)
                    {
                        return Some(ApiType::Delegate(
                            ApiTypeDelegate::ZeroCopyBufferVecPrimitive(primitive),
                        ));
                    }
                }

                None
            }
        }
    }

    fn try_parse_list(&mut self, ty: &str) -> Option<ApiType> {
        lazy_static! {
            static ref CAPTURE_VEC: GenericCapture = GenericCapture::new("Vec");
        }

        if let Some(inner_type_str) = CAPTURE_VEC.captures(ty) {
            match self.parse_type(&inner_type_str) {
                Primitive(primitive) => Some(PrimitiveList(ApiTypePrimitiveList { primitive })),
                others => Some(GeneralList(Box::from(ApiTypeGeneralList { inner: others }))),
            }
        } else {
            None
        }
    }

    fn try_parse_box(&mut self, ty: &str) -> Option<ApiType> {
        lazy_static! {
            static ref CAPTURE_BOX: GenericCapture = GenericCapture::new("Box");
        }

        CAPTURE_BOX.captures(ty).map(|inner| {
            Boxed(Box::new(ApiTypeBoxed {
                exist_in_real_api: true,
                inner: self.parse_type(&inner),
            }))
        })
    }

    fn try_parse_option(&mut self, ty: &str) -> Option<ApiType> {
        lazy_static! {
            static ref CAPTURE_OPTION: GenericCapture = GenericCapture::new("Option");
        }

        CAPTURE_OPTION.captures(ty).map(|inner| {
            let inner_option = CAPTURE_OPTION.captures(&inner);
            if let Some(inner_option) = inner_option {
                panic!(
                    "Nested optionals without indirection are not supported. (Option<Option<{}>>)",
                    inner_option
                );
            };
            match self.parse_type(&inner) {
                Primitive(prim) => ApiType::Optional(ApiTypeOptional::new_prim(prim)),
                st @ StructRef(_) => {
                    ApiType::Optional(ApiTypeOptional::new_ptr(Boxed(Box::new(ApiTypeBoxed {
                        inner: st,
                        exist_in_real_api: false,
                    }))))
                }
                other => ApiType::Optional(ApiTypeOptional::new_ptr(other)),
            }
        })
    }

    fn try_parse_struct(&mut self, ty: &str) -> Option<ApiType> {
        if !self.src_struct_map.contains_key(ty) {
            return None;
        }

        if !self.parsing_or_parsed_struct_names.contains(ty) {
            self.parsing_or_parsed_struct_names.insert(ty.to_string());
            let api_struct = self.parse_struct_core(ty);
            self.struct_pool.insert(ty.to_string(), api_struct);
        }

        Some(StructRef(ApiTypeStructRef {
            name: ty.to_string(),
        }))
    }

    fn parse_struct_core(&mut self, ty: &str) -> ApiStruct {
        let item_struct = self.src_struct_map[ty];
        let mut fields = Vec::new();

        let (is_fields_named, struct_fields) = match &item_struct.fields {
            Fields::Named(FieldsNamed { named, .. }) => (true, named),
            Fields::Unnamed(FieldsUnnamed { unnamed, .. }) => (false, unnamed),
            _ => panic!("unsupported type: {:?}", item_struct.fields),
        };

        for (idx, field) in struct_fields.iter().enumerate() {
            let field_name = field
                .ident
                .as_ref()
                .map_or(format!("field{}", idx), |id| ident_to_string(id));
            let field_type_str = type_to_string(&field.ty);
            let field_type = self.parse_type(&field_type_str);
            let comments = field.attrs.iter().filter_map(extract_comments).collect();
            fields.push(ApiField {
                name: ApiIdent::new(field_name),
                ty: field_type,
                comments,
            });
        }

        let name = ident_to_string(&item_struct.ident);
        let comments = item_struct
            .attrs
            .iter()
            .filter_map(extract_comments)
            .collect();
        ApiStruct {
            name,
            fields,
            is_fields_named,
            comments,
        }
    }
}

fn extract_items_from_file(file: &File) -> (Vec<&ItemFn>, StructMap) {
    let mut src_fns = Vec::new();
    let mut src_struct_map = HashMap::new();
    for item in file.items.iter() {
        match item {
            Item::Fn(ref item_fn) => {
                if let Visibility::Public(_) = &item_fn.vis {
                    src_fns.push(item_fn);
                }
            }
            Item::Struct(ref item_struct) => {
                if let Visibility::Public(_) = &item_struct.vis {
                    src_struct_map.insert(item_struct.ident.to_string(), item_struct);
                }
            }
            _ => {}
        }
    }
    // println!("[Functions]\n{:#?}", src_fns);
    // println!("[Structs]\n{:#?}", src_struct_map);
    (src_fns, src_struct_map)
}

fn ident_to_string(ident: &Ident) -> String {
    format!("{}", ident)
}

/// syn -> string https://github.com/dtolnay/syn/issues/294
fn type_to_string(ty: &Type) -> String {
    quote!(#ty).to_string().replace(" ", "")
}

struct GenericCapture {
    regex: Regex,
}

impl GenericCapture {
    pub fn new(cls_name: &str) -> Self {
        let regex = Regex::new(&*format!("^[^<]*{}<([a-zA-Z0-9_<>]+)>$", cls_name)).unwrap();
        Self { regex }
    }

    /// e.g. List<Tom> => return Some(Tom)
    pub fn captures(&self, s: &str) -> Option<String> {
        self.regex
            .captures(s)
            .map(|capture| capture.get(1).unwrap().as_str().to_string())
    }
}
